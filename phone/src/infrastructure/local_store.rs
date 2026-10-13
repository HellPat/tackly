//! The device's append-only event log in SQLite. Event bodies are encrypted
//! under a device-local key; visible state is rebuilt by replaying them.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use tackly_domain::FamilyEvent;
use uuid::Uuid;

use crate::{
    infrastructure::crypto::{SealedData, open, random_bytes, seal},
    infrastructure::secrets::Secrets,
};

/// The event log shared between the device and its `cqrs-es` store.
#[derive(Clone)]
pub struct SharedStore(Arc<Mutex<EventStore>>);

impl SharedStore {
    pub fn new(store: EventStore) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    /// A poisoned lock means another thread panicked while holding it. Each
    /// operation on the store is one SQLite statement or transaction, so the
    /// data is intact and work continues.
    pub fn lock(&self) -> MutexGuard<'_, EventStore> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct EventStore {
    db: Connection,
    local_key: Vec<u8>,
    pub device_id: Uuid,
}

impl EventStore {
    pub fn open(path: &Path, secrets: &Secrets) -> Result<Self> {
        std::fs::create_dir_all(path.parent().context("no folder for the event database")?)?;
        let db = Connection::open(path).context("open Tackly event database")?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS events (
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,
               event_id TEXT NOT NULL UNIQUE,
               schema_version INTEGER NOT NULL,
               aggregate_id TEXT NOT NULL,
               occurred_at_utc TEXT NOT NULL,
               origin_device_id TEXT NOT NULL,
               local_nonce TEXT NOT NULL,
               local_ciphertext TEXT NOT NULL,
               local_mac TEXT NOT NULL,
               server_sequence INTEGER,
               pushed INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS sync_metadata (
               name TEXT PRIMARY KEY, value TEXT NOT NULL
             );",
        )?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        let local_key = match secrets.read("local_event_key")? {
            Some(key) => {
                ensure!(key.len() == 32, "invalid local event key");
                key
            }
            None if count == 0 => {
                let key = random_bytes::<32>().to_vec();
                secrets.write("local_event_key", &key)?;
                key
            }
            None => bail!("local events exist but the device key is missing"),
        };
        let device_id = match secrets.read("device_id")? {
            Some(bytes) => String::from_utf8(bytes)?.parse().context("device ID")?,
            None if count == 0 => {
                let id = Uuid::new_v4();
                secrets.write("device_id", id.to_string().as_bytes())?;
                id
            }
            None => bail!("local events exist but the device identity is missing"),
        };
        Ok(Self {
            db,
            local_key,
            device_id,
        })
    }

    pub fn insert(&self, event: &FamilyEvent, server_sequence: Option<i64>) -> Result<()> {
        let id = event.id.to_string();
        let envelope = seal(&self.local_key, &serde_json::to_vec(event)?, &id)?;
        // Known already (the relay sends our own events back): only what the
        // relay says about it changes.
        self.db.execute(
            "INSERT INTO events
             (event_id, schema_version, aggregate_id, occurred_at_utc, origin_device_id,
              local_nonce, local_ciphertext, local_mac, server_sequence, pushed)
             VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (event_id) DO UPDATE
               SET server_sequence = excluded.server_sequence, pushed = 1
               WHERE excluded.server_sequence IS NOT NULL",
            params![
                id,
                event.subject_id.to_string(),
                event.occurred_at.to_rfc3339(),
                event.origin_device_id.to_string(),
                envelope.nonce,
                envelope.ciphertext,
                envelope.mac,
                server_sequence,
                i64::from(server_sequence.is_some()),
            ],
        )?;
        Ok(())
    }

    /// Server-confirmed events in server order, then local pending events.
    pub fn read_events(&self) -> Result<Vec<FamilyEvent>> {
        self.read_where("1=1", [])
    }

    pub fn pending_events(&self) -> Result<Vec<FamilyEvent>> {
        self.read_where(
            "pushed = 0 AND origin_device_id = ?",
            [self.device_id.to_string()],
        )
    }

    fn read_where(&self, filter: &str, args: impl rusqlite::Params) -> Result<Vec<FamilyEvent>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT event_id, schema_version, local_nonce, local_ciphertext, local_mac
             FROM events WHERE {filter}
             ORDER BY server_sequence IS NULL, server_sequence ASC, sequence ASC"
        ))?;
        let rows = statement.query_map(args, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                SealedData {
                    nonce: row.get(2)?,
                    ciphertext: row.get(3)?,
                    mac: row.get(4)?,
                },
            ))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (id, version, sealed) = row?;
            ensure!(version == 1, "unsupported event schema version {version}");
            let plaintext = open(&self.local_key, &sealed, &id)?;
            events.push(serde_json::from_slice(&plaintext)?);
        }
        Ok(events)
    }

    pub fn cursor(&self) -> Result<i64> {
        let cursor = self
            .db
            .query_row(
                "SELECT value FROM sync_metadata WHERE name='cursor'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(cursor.map(|value| value.parse()).transpose()?.unwrap_or(0))
    }

    pub fn advance_cursor(&self, cursor: i64) -> Result<()> {
        if cursor > self.cursor()? {
            self.db.execute(
                "INSERT INTO sync_metadata (name,value) VALUES ('cursor',?)
                 ON CONFLICT(name) DO UPDATE SET value=excluded.value",
                [cursor.to_string()],
            )?;
        }
        Ok(())
    }

    /// The relay stored one of this phone's own events at `sequence`.
    pub fn confirm(&self, event_id: Uuid, sequence: i64) -> Result<()> {
        self.db.execute(
            "UPDATE events SET server_sequence=?, pushed=1 WHERE event_id=?",
            params![sequence, event_id.to_string()],
        )?;
        Ok(())
    }

    pub fn mark_pushed(&self, event_id: Uuid) -> Result<()> {
        self.db.execute(
            "UPDATE events SET pushed=1 WHERE event_id=?",
            [event_id.to_string()],
        )?;
        Ok(())
    }

    /// Takes over another device's identity (the head's lost phone, from its
    /// backup). Only before anything was recorded here.
    pub fn adopt_identity(&mut self, device_id: Uuid, secrets: &Secrets) -> Result<()> {
        ensure!(self.is_empty()?, "this phone already has a history");
        secrets.write("device_id", device_id.to_string().as_bytes())?;
        self.device_id = device_id;
        Ok(())
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count == 0)
    }

    /// Forget all events and this device's identity.
    pub fn clear(&mut self, secrets: &Secrets) -> Result<()> {
        self.db.execute_batch(
            "PRAGMA secure_delete=ON;
             DELETE FROM events;
             DELETE FROM sync_metadata;
             VACUUM;",
        )?;
        self.local_key = random_bytes::<32>().to_vec();
        self.device_id = Uuid::new_v4();
        secrets.write("local_event_key", &self.local_key)?;
        secrets.write("device_id", self.device_id.to_string().as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use tackly_domain::DomainEvent;

    use super::*;

    fn started(by: Uuid) -> FamilyEvent {
        FamilyEvent {
            id: Uuid::new_v4(),
            subject_id: Uuid::new_v4(),
            origin_device_id: by,
            occurred_at: Utc::now(),
            event: DomainEvent::TaskStarted,
        }
    }

    fn store(dir: &tempfile::TempDir) -> (EventStore, Secrets) {
        let secrets = Secrets::new(dir.path());
        let store = EventStore::open(&dir.path().join("tackly.db"), &secrets).unwrap();
        (store, secrets)
    }

    #[test]
    fn a_file_that_is_no_database_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tackly.db");
        std::fs::write(&path, b"not a database at all, not even close").unwrap();
        assert!(EventStore::open(&path, &Secrets::new(dir.path())).is_err());
        assert!(EventStore::open(Path::new("/"), &Secrets::new(dir.path())).is_err());
    }

    #[test]
    fn events_without_their_key_or_identity_are_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let (store, secrets) = store(&dir);
        store.insert(&started(store.device_id), None).unwrap();
        let key = secrets.read("local_event_key").unwrap().unwrap();
        drop(store);

        secrets.delete("local_event_key").unwrap();
        let missing_key = EventStore::open(&dir.path().join("tackly.db"), &secrets);
        assert!(
            missing_key
                .err()
                .unwrap()
                .to_string()
                .contains("device key is missing")
        );

        secrets.write("local_event_key", &key[..31]).unwrap();
        let short_key = EventStore::open(&dir.path().join("tackly.db"), &secrets);
        assert!(
            short_key
                .err()
                .unwrap()
                .to_string()
                .contains("invalid local event key")
        );

        secrets.write("local_event_key", &key).unwrap();
        secrets.delete("device_id").unwrap();
        let missing_id = EventStore::open(&dir.path().join("tackly.db"), &secrets);
        assert!(
            missing_id
                .err()
                .unwrap()
                .to_string()
                .contains("identity is missing")
        );
    }

    #[test]
    fn when_the_database_refuses_to_write_every_change_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, secrets) = store(&dir);
        let event = started(store.device_id);
        store.insert(&event, None).unwrap();
        store.db.execute_batch("PRAGMA query_only = ON").unwrap();

        assert!(store.insert(&started(store.device_id), None).is_err());
        assert!(store.insert(&event, Some(1)).is_err());
        assert!(store.confirm(event.id, 1).is_err());
        assert!(store.mark_pushed(event.id).is_err());
        assert!(store.advance_cursor(5).is_err());
        assert!(store.clear(&secrets).is_err());
    }
}
