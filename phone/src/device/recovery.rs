//! The head's way back after losing its phone. The recovery code is an age
//! identity (see [`RecoveryCode`]); the head's phone seals a backup to its
//! public key (age) and leaves it with the relay, after every change, under an
//! ID made from that public key. The phone forgets the code itself as soon as
//! the head said it saved it: sealing needs only the public key. A new phone
//! with the code fetches and opens the backup, takes over the head's identity,
//! signing key and history, comes back into the MLS group from outside (an
//! external commit, which also removes the lost phone's leaf), moves to a
//! fresh signing key and a fresh relay token, and the other phones send it
//! what happened since the backup.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tackly_domain::{FamilyEvent, Name};
use tackly_wire::{Bytes, EncryptedEvent};
use uuid::Uuid;

use age::x25519::Recipient;

use super::recovery_code::{RecoveryCode, backup_id, seal};
use super::{Device, Membership};
use crate::infrastructure::{
    crypto, device_token::DeviceToken, secrets::Secrets, server_url::ServerUrl,
};

/// The code, until the head said it saved it.
const SECRET: &str = "recovery-secret";
/// The code's public key: what backups are sealed to.
const RECIPIENT: &str = "recovery-recipient";
const BACKUP_VERSION: u32 = 1;

/// What the backup holds. The events are the whole history, so a family of
/// one phone survives too.
#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    server_url: ServerUrl,
    family_id: Uuid,
    device_id: Uuid,
    device_token: DeviceToken,
    name: Name,
    /// The MLS signing key (serialized) and the group's public state.
    signer: String,
    group_info: String,
    /// The relay cursor at backup time.
    cursor: i64,
    events: Vec<FamilyEvent>,
}

/// The code the phone still keeps because the head has not yet said it saved
/// it. An older format (24 words, or a short code) reads as none.
fn unsaved_code(secrets: &Secrets) -> Result<Option<RecoveryCode>> {
    Ok(secrets
        .read(SECRET)?
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse().ok()))
}

/// The public key backups are sealed to (none: no recovery code yet).
fn kept_recipient(secrets: &Secrets) -> Result<Option<Recipient>> {
    Ok(secrets
        .read(RECIPIENT)?
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse().ok()))
}

impl Device {
    /// The recovery code the head has not confirmed to have saved yet (the
    /// app shows it until it is). `None` for everyone else.
    pub fn pending_recovery_code(&self) -> Result<Option<RecoveryCode>> {
        if !self.membership.as_ref().is_some_and(|m| m.owner) {
            return Ok(None);
        }
        unsaved_code(&self.secrets)
    }

    /// The head says it saved the code: the phone forgets it, and keeps the
    /// public key that backups are sealed to.
    pub fn confirm_recovery_code(&mut self) -> Result<()> {
        self.secrets.delete(SECRET)
    }

    /// The head's first recovery code, made without asking the relay: it is
    /// backed up as soon as the relay can be reached.
    pub(super) async fn ensure_recovery_code(&mut self) -> Result<()> {
        if !self.membership.as_ref().is_some_and(|m| m.owner)
            || kept_recipient(&self.secrets)?.is_some()
        {
            return Ok(());
        }
        let code = RecoveryCode::generate();
        self.keep(&code)?;
        self.backed_up = None;
        // Best effort: `flush` tries again whenever the backup is out of date.
        let _ = self.upload_backup(&code.recipient()).await;
        Ok(())
    }

    /// Keeps a new code until it is saved, and its public key for good.
    fn keep(&self, code: &RecoveryCode) -> Result<()> {
        self.secrets.write(SECRET, code.to_string().as_bytes())?;
        self.secrets
            .write(RECIPIENT, code.recipient().to_string().as_bytes())
    }

    /// Makes a new recovery code and backs the family up under it. The
    /// previous backup, if any, is removed: its code is no good any more.
    /// The new code must be confirmed again.
    pub async fn new_recovery_code(&mut self) -> Result<RecoveryCode> {
        let membership = self.joined()?.clone();
        ensure!(
            membership.owner,
            "only the head of the family has a recovery code"
        );
        self.flush().await?;
        let code = RecoveryCode::generate();
        // The first backup must go through before the old one goes.
        self.upload_backup(&code.recipient()).await?;
        if let Some(old) = kept_recipient(&self.secrets)? {
            let _ = self
                .api()?
                .delete_recovery(
                    membership.family_id,
                    &membership.device_token,
                    &backup_id(&old),
                )
                .await;
        }
        self.keep(&code)?;
        self.backed_up = Some(self.backup_mark()?);
        Ok(code)
    }

    /// What the backup covers: the number of events and the group's epoch.
    fn backup_mark(&self) -> Result<(usize, u64)> {
        let family_id = self.joined()?.family_id;
        Ok((
            self.events.lock().read_events()?.len(),
            self.mls.epoch(family_id)?,
        ))
    }

    /// Backs the family up when it changed since the last time (the head's
    /// phone, after every flush).
    pub(super) async fn backup(&mut self) -> Result<()> {
        // Only the head keeps a recovery key, so only the head backs up.
        let Some(recipient) = kept_recipient(&self.secrets)? else {
            return Ok(());
        };
        let mark = self.backup_mark()?;
        if self.backed_up == Some(mark) {
            return Ok(());
        }
        self.upload_backup(&recipient).await?;
        self.backed_up = Some(mark);
        Ok(())
    }

    async fn upload_backup(&mut self, recipient: &Recipient) -> Result<()> {
        let membership = self.joined()?.clone();
        // One lock at a time: a guard in a struct literal lives to its end.
        let cursor = self.events.lock().cursor()?;
        let events = self.events.lock().read_events()?;
        let backup = Backup {
            version: BACKUP_VERSION,
            server_url: membership.server_url.clone(),
            family_id: membership.family_id,
            device_id: self.device_id(),
            device_token: membership.device_token.clone(),
            name: membership.name.clone(),
            signer: crypto::encode(&self.mls.export_signer()?),
            group_info: crypto::encode(&self.mls.export_group_info(membership.family_id)?),
            cursor,
            events,
        };
        let blob = seal(recipient, &serde_json::to_vec(&backup)?)?;
        self.api()?
            .put_recovery(
                membership.family_id,
                &membership.device_token,
                &backup_id(recipient),
                &blob,
            )
            .await
    }

    /// A new phone becomes the head again, from the backup the code opens.
    pub async fn restore(&mut self, server_url: &ServerUrl, code: &RecoveryCode) -> Result<()> {
        ensure!(self.membership.is_none(), "already in a family");
        ensure!(
            self.events.lock().is_empty()?,
            "this phone already has a history"
        );
        let api = crate::infrastructure::relay_client::Api::new(server_url)?;
        let blob = api
            .get_recovery(&code.backup_id())
            .await
            .context("no backup for this recovery code on that server")?;
        let opened = code
            .open(&blob)
            .map_err(|_| anyhow::anyhow!("the recovery code does not fit that backup"))?;
        let backup: Backup = serde_json::from_slice(&opened).context("the backup is damaged")?;
        ensure!(
            backup.version == BACKUP_VERSION,
            "the backup is from another version of Tackly"
        );
        if let Err(error) = self.take_over(&api, backup).await {
            // Half a restore is no good: start over.
            let _ = self.logout();
            return Err(error);
        }
        // Only now the code is ours: backups continue, sealed to its public
        // key. The person just typed it, so there is nothing to save.
        self.secrets
            .write(RECIPIENT, code.recipient().to_string().as_bytes())?;
        self.backup().await?;
        Ok(())
    }

    async fn take_over(
        &mut self,
        api: &crate::infrastructure::relay_client::Api,
        backup: Backup,
    ) -> Result<()> {
        let family_id = backup.family_id;
        {
            let mut store = self.events.lock();
            store.adopt_identity(backup.device_id, &self.secrets)?;
            for event in &backup.events {
                // Everything here is known to the relay already.
                store.insert(event, Some(0))?;
            }
            store.advance_cursor(backup.cursor)?;
        }
        self.cqrs = Self::framework(&self.events, &self.upload);
        self.mls
            .adopt_signer(&crypto::decode(&backup.signer)?, backup.device_id)?;
        let me = backup.device_id;
        let commit = self
            .mls
            .rejoin(family_id, &crypto::decode(&backup.group_info)?)?;
        api.append(family_id, &backup.device_token, vec![message(me, commit)])
            .await
            .context("come back into the family's group")?;
        self.save_membership(Membership {
            server_url: api.base().clone(),
            family_id,
            head: me,
            device_token: backup.device_token.clone(),
            registered: true,
            owner: true,
            name: backup.name,
            agent: false,
            removed: false,
            restoring: true,
        })?;
        // A fresh signing key: the lost phone's is no longer the head's.
        let rotation = self.mls.rotate_signer(family_id, me)?;
        api.append(family_id, &backup.device_token, vec![message(me, rotation)])
            .await
            .context("move to a fresh signing key")?;
        self.mls.merge_rotation(family_id, me)?;
        // A fresh token: the lost phone cannot talk to the relay any more.
        let token = DeviceToken::generate();
        api.rotate_token(family_id, &backup.device_token, &token)
            .await
            .context("move to a fresh relay token")?;
        let membership = self.joined()?.clone();
        self.save_membership(Membership {
            device_token: token,
            ..membership
        })?;
        self.pull().await?;
        Ok(())
    }
}

fn message(origin: Uuid, bytes: Vec<u8>) -> EncryptedEvent {
    EncryptedEvent {
        event_id: Uuid::new_v4(),
        origin_device_id: origin,
        message: Bytes::from(bytes),
    }
}
