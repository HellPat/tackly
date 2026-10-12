//! The head's way back after losing its phone. A recovery code (160 random
//! bits) gives a key and an ID (HKDF); the head's phone seals a backup
//! with the key (AES-256-GCM) and leaves it with the relay under the ID,
//! after every change. A new phone with the code fetches and opens it,
//! takes over the head's identity, signing key and history, comes back into
//! the MLS group from outside (an external commit, which also removes the
//! lost phone's leaf), moves to a fresh signing key and a fresh relay token,
//! and the other phones send it what happened since the backup.

use anyhow::{Context, Result, ensure};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tackly_domain::{FamilyEvent, Name};
use tackly_wire::{Bytes, Digest, EncryptedEvent};
use uuid::Uuid;

use super::recovery_code::RecoveryCode;
use super::{Device, Membership};
use crate::infrastructure::{
    crypto::{self},
    device_token::DeviceToken,
    server_url::ServerUrl,
};

const WORDS: &str = "recovery-words";
/// Which code the head confirmed to have saved (a hash of it).
const CONFIRMED: &str = "recovery-confirmed";
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

/// The key and the ID a recovery code stands for.
pub struct RecoveryKey {
    id: [u8; 32],
    key: [u8; 32],
}

impl RecoveryKey {
    /// The key and the backup's ID the code stands for.
    pub fn new(code: &RecoveryCode) -> Result<Self> {
        // The code is already random and long: no stretching needed.
        let hkdf = Hkdf::<Sha256>::new(Some(b"tackly-recovery-code-v2"), code.secret());
        let (mut id, mut key) = ([0; 32], [0; 32]);
        hkdf.expand(b"id", &mut id)
            .map_err(|_| anyhow::anyhow!("derive the recovery ID"))?;
        hkdf.expand(b"key", &mut key)
            .map_err(|_| anyhow::anyhow!("derive the recovery key"))?;
        Ok(Self { id, key })
    }

    /// Where the relay keeps the backup (tells nothing about the code).
    pub fn id(&self) -> Digest {
        Digest::from(self.id)
    }

    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&crypto::seal(
            &self.key,
            plaintext,
            &self.id().to_string(),
        )?)?)
    }

    fn open(&self, blob: &[u8]) -> Result<Vec<u8>> {
        let sealed = serde_json::from_slice(blob).context("the backup is damaged")?;
        crypto::open(&self.key, &sealed, &self.id().to_string())
            .context("the recovery code does not fit")
    }
}

/// What the phone keeps to know the head confirmed the code: a hash of it.
fn confirmation_of(code: &RecoveryCode) -> String {
    crypto::encode(&crypto::derive(
        "recovery-confirmed",
        &[code.to_string().as_bytes()],
    ))
}

/// The code the phone keeps (the backup continues under it). One in an older
/// format (24 words) reads as none: a new code is made in its place.
fn kept_code(secrets: &crate::infrastructure::secrets::Secrets) -> Result<Option<RecoveryCode>> {
    Ok(secrets
        .read(WORDS)?
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| text.parse::<RecoveryCode>().ok()))
}

impl Device {
    /// The recovery code the head has not confirmed to have saved yet (the
    /// app shows it until it is). `None` for everyone else.
    pub fn pending_recovery_code(&self) -> Result<Option<RecoveryCode>> {
        if !self.membership.as_ref().is_some_and(|m| m.owner) {
            return Ok(None);
        }
        let Some(code) = kept_code(&self.secrets)? else {
            return Ok(None);
        };
        let confirmed = self.secrets.read(CONFIRMED)?;
        Ok((confirmed.as_deref() != Some(confirmation_of(&code).as_bytes())).then_some(code))
    }

    /// The head says it saved the code.
    pub fn confirm_recovery_code(&mut self) -> Result<()> {
        let Some(code) = kept_code(&self.secrets)? else {
            return Ok(());
        };
        self.secrets
            .write(CONFIRMED, confirmation_of(&code).as_bytes())
    }

    /// The head's first recovery code, made without asking the relay: it is
    /// backed up as soon as the relay can be reached.
    pub(super) async fn ensure_recovery_code(&mut self) -> Result<()> {
        if !self.membership.as_ref().is_some_and(|m| m.owner) || kept_code(&self.secrets)?.is_some()
        {
            return Ok(());
        }
        let code = RecoveryCode::generate();
        let key = RecoveryKey::new(&code)?;
        self.secrets.write(WORDS, code.to_string().as_bytes())?;
        self.secrets.delete(CONFIRMED)?;
        self.backed_up = None;
        // Best effort: `flush` tries again whenever the backup is out of date.
        let _ = self.upload_backup(&key).await;
        Ok(())
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
        let key = RecoveryKey::new(&code)?;
        // The first backup must go through before the old one goes.
        self.upload_backup(&key).await?;
        if let Some(old) = kept_code(&self.secrets)? {
            let _ = self
                .api()?
                .delete_recovery(
                    membership.family_id,
                    &membership.device_token,
                    &RecoveryKey::new(&old)?.id(),
                )
                .await;
        }
        self.secrets.write(WORDS, code.to_string().as_bytes())?;
        self.secrets.delete(CONFIRMED)?;
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
        let Some(code) = kept_code(&self.secrets)? else {
            return Ok(());
        };
        if !self.joined()?.owner {
            return Ok(());
        }
        let mark = self.backup_mark()?;
        if self.backed_up == Some(mark) {
            return Ok(());
        }
        let key = RecoveryKey::new(&code)?;
        self.upload_backup(&key).await?;
        self.backed_up = Some(mark);
        Ok(())
    }

    async fn upload_backup(&mut self, key: &RecoveryKey) -> Result<()> {
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
        let blob = key.seal(&serde_json::to_vec(&backup)?)?;
        self.api()?
            .put_recovery(
                membership.family_id,
                &membership.device_token,
                &key.id(),
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
        let key = RecoveryKey::new(code)?;
        let blob = api
            .get_recovery(&key.id())
            .await
            .context("no backup for this recovery code on that server")?;
        let backup: Backup =
            serde_json::from_slice(&key.open(&blob)?).context("the backup is damaged")?;
        ensure!(
            backup.version == BACKUP_VERSION,
            "the backup is from another version of Tackly"
        );
        if let Err(error) = self.take_over(&api, backup).await {
            // Half a restore is no good: start over.
            let _ = self.logout();
            return Err(error);
        }
        // Only now the code is ours: backups continue under it. The person
        // just typed them, so there is nothing to save.
        self.secrets.write(WORDS, code.to_string().as_bytes())?;
        self.secrets
            .write(CONFIRMED, confirmation_of(code).as_bytes())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_code_gives_the_same_key_and_another_does_not() {
        let code = RecoveryCode::generate();
        let key = RecoveryKey::new(&code).expect("key");
        let typed = format!("  {}  ", code.to_string().to_lowercase().replace('-', " "));
        let again = RecoveryKey::new(&typed.parse().expect("the same code")).expect("key again");
        assert_eq!(key.id(), again.id());
        let sealed = key.seal(b"family").expect("seal");
        assert_eq!(again.open(&sealed).expect("open"), b"family");
        let other = RecoveryKey::new(&RecoveryCode::generate()).expect("key");
        assert!(other.open(&sealed).is_err());
    }
}
