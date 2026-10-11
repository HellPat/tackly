//! The head's way back after losing its phone. A recovery code (24 words,
//! BIP-39) gives a key and an ID (HKDF); the head's phone seals a backup
//! with the key (AES-256-GCM) and leaves it with the relay under the ID,
//! after every change. A new phone with the words fetches and opens it,
//! takes over the head's identity, signing key and history, comes back into
//! the MLS group from outside (an external commit, which also removes the
//! lost phone's leaf), moves to a fresh signing key and a fresh relay token,
//! and the other phones send it what happened since the backup.

use anyhow::{Context, Result, ensure};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tackly_domain::FamilyEvent;
use tackly_wire::EncryptedEvent;
use uuid::Uuid;

use super::{Device, Membership};
use crate::infrastructure::crypto::{self, random_bytes};

const WORDS: &str = "recovery-words";
const BACKUP_VERSION: u32 = 1;

/// What the backup holds. The events are the whole history, so a family of
/// one phone survives too.
#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    server_url: String,
    family_id: Uuid,
    device_id: Uuid,
    device_token: String,
    name: String,
    /// The MLS signing key (serialized) and the group's public state.
    signer: String,
    group_info: String,
    /// The relay cursor at backup time.
    cursor: i64,
    events: Vec<FamilyEvent>,
}

/// The key and the ID the recovery words stand for.
pub struct RecoveryKey {
    id: [u8; 32],
    key: [u8; 32],
}

impl RecoveryKey {
    /// Fresh words (24, from 32 random bytes) and their key.
    pub fn generate() -> Result<(String, Self)> {
        let mnemonic = bip39::Mnemonic::from_entropy(&random_bytes::<32>())
            .context("make the recovery words")?;
        let words = mnemonic.to_string();
        Ok((words.clone(), Self::from_words(&words)?))
    }

    pub fn from_words(words: &str) -> Result<Self> {
        let mnemonic = bip39::Mnemonic::parse_normalized(&normalize(words))
            .map_err(|_| anyhow::anyhow!("that is not a recovery code of 24 words"))?;
        ensure!(mnemonic.word_count() == 24, "a recovery code has 24 words");
        let seed = mnemonic.to_seed("");
        let hkdf = Hkdf::<Sha256>::new(Some(b"tackly-recovery"), &seed);
        let (mut id, mut key) = ([0; 32], [0; 32]);
        hkdf.expand(b"id", &mut id)
            .map_err(|_| anyhow::anyhow!("derive the recovery ID"))?;
        hkdf.expand(b"key", &mut key)
            .map_err(|_| anyhow::anyhow!("derive the recovery key"))?;
        Ok(Self { id, key })
    }

    /// Where the relay keeps the backup (tells nothing about the words).
    pub fn id(&self) -> String {
        crypto::encode(&self.id)
    }

    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&crypto::seal(
            &self.key,
            plaintext,
            &self.id(),
        )?)?)
    }

    fn open(&self, blob: &[u8]) -> Result<Vec<u8>> {
        let sealed = serde_json::from_slice(blob).context("the backup is damaged")?;
        crypto::open(&self.key, &sealed, &self.id()).context("the recovery code does not fit")
    }
}

/// Lower case, single spaces: what a person typed or pasted.
fn normalize(words: &str) -> String {
    words
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

impl Device {
    /// Whether the head's phone keeps a backup.
    pub fn has_recovery_code(&self) -> Result<bool> {
        Ok(self.secrets.read(WORDS)?.is_some())
    }

    /// Makes (new) recovery words and backs the family up under them. The
    /// previous backup, if any, is removed: its words are no good any more.
    pub async fn new_recovery_code(&mut self) -> Result<String> {
        let membership = self.joined()?.clone();
        ensure!(
            membership.owner,
            "only the head of the family has a recovery code"
        );
        self.flush().await?;
        let (words, key) = RecoveryKey::generate()?;
        // The first backup must go through before the old one goes.
        self.upload_backup(&key).await?;
        if let Some(old) = self.secrets.read(WORDS)?
            && let Ok(old) = RecoveryKey::from_words(&String::from_utf8_lossy(&old))
        {
            let _ = self
                .api()?
                .delete_recovery(membership.family_id, &membership.device_token, &old.id())
                .await;
        }
        self.secrets.write(WORDS, words.as_bytes())?;
        self.backed_up = Some(self.backup_mark()?);
        Ok(words)
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
        let Some(words) = self.secrets.read(WORDS)? else {
            return Ok(());
        };
        if !self.joined()?.owner {
            return Ok(());
        }
        let mark = self.backup_mark()?;
        if self.backed_up == Some(mark) {
            return Ok(());
        }
        let key = RecoveryKey::from_words(&String::from_utf8_lossy(&words))?;
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

    /// A new phone becomes the head again, from the backup the words open.
    pub async fn restore(&mut self, server_url: &str, words: &str) -> Result<()> {
        ensure!(self.membership.is_none(), "already in a family");
        ensure!(
            self.events.lock().is_empty()?,
            "this phone already has a history"
        );
        let api = crate::infrastructure::relay_client::Api::new(server_url)?;
        let key = RecoveryKey::from_words(words)?;
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
        // Only now the words are ours: backups continue under them.
        self.secrets.write(WORDS, normalize(words).as_bytes())?;
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
            server_url: api.base().to_owned(),
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
        let token = crypto::encode(&random_bytes::<32>());
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
        message: crypto::encode(&bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_give_the_same_key_however_they_are_typed() {
        let (words, key) = RecoveryKey::generate().expect("generate");
        assert_eq!(words.split(' ').count(), 24);
        let typed = format!("  {}  ", words.to_uppercase().replace(' ', "\n"));
        let again = RecoveryKey::from_words(&typed).expect("parse");
        assert_eq!(key.id(), again.id());
        let sealed = key.seal(b"family").expect("seal");
        assert_eq!(again.open(&sealed).expect("open"), b"family");
        let (_, other) = RecoveryKey::generate().expect("generate");
        assert!(other.open(&sealed).is_err());
        assert!(RecoveryKey::from_words("just a few words").is_err());
    }
}
