//! The device's append-only event log in SQLite. Event bodies are encrypted
//! under a device-local key; visible state is rebuilt by replaying them.

use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use tackly_protocol::{FamilyEvent, wire::EncryptedEvent};
use uuid::Uuid;

use crate::{
    crypto::{SealedData, open, random_bytes, seal},
    secrets::Secrets,
};

pub struct EventStore {
    db: Connection,
    local_key: Vec<u8>,
    pub device_id: Uuid,
}

impl EventStore {
    pub fn open(path: &Path, secrets: &Secrets) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
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
             );
             CREATE TABLE IF NOT EXISTS sync_outbox (
               event_id TEXT PRIMARY KEY REFERENCES events(event_id),
               nonce TEXT NOT NULL, ciphertext TEXT NOT NULL, mac TEXT NOT NULL
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
        self.db.execute(
            "INSERT OR IGNORE INTO events
             (event_id, schema_version, aggregate_id, occurred_at_utc, origin_device_id,
              local_nonce, local_ciphertext, local_mac, server_sequence, pushed)
             VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        if let Some(sequence) = server_sequence {
            self.db.execute(
                "UPDATE events SET server_sequence=?, pushed=1 WHERE event_id=?",
                params![sequence, id],
            )?;
        }
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

    /// The upload ciphertext is created once and reused for retries, so a
    /// retried POST is byte-identical and idempotent on the server.
    pub fn outbox_envelope(
        &self,
        event: &FamilyEvent,
        family_id: Uuid,
        family_key: &[u8],
    ) -> Result<EncryptedEvent> {
        let id = event.id.to_string();
        let lookup = || {
            self.db
                .query_row(
                    "SELECT nonce, ciphertext, mac FROM sync_outbox WHERE event_id=?",
                    [&id],
                    |row| {
                        Ok(SealedData {
                            nonce: row.get(0)?,
                            ciphertext: row.get(1)?,
                            mac: row.get(2)?,
                        })
                    },
                )
                .optional()
        };
        let mut wire = EncryptedEvent {
            event_id: event.id,
            aggregate_id: event.subject_id,
            origin_device_id: event.origin_device_id,
            key_version: 1,
            nonce: String::new(),
            ciphertext: String::new(),
            mac: String::new(),
        };
        let sealed = match lookup()? {
            Some(sealed) => sealed,
            None => {
                let sealed = seal(
                    family_key,
                    &serde_json::to_vec(event)?,
                    &wire.associated_data(family_id),
                )?;
                self.db.execute(
                    "INSERT OR IGNORE INTO sync_outbox (event_id, nonce, ciphertext, mac)
                     VALUES (?, ?, ?, ?)",
                    params![id, sealed.nonce, sealed.ciphertext, sealed.mac],
                )?;
                lookup()?.context("outbox row")?
            }
        };
        wire.nonce = sealed.nonce;
        wire.ciphertext = sealed.ciphertext;
        wire.mac = sealed.mac;
        Ok(wire)
    }

    pub fn mark_pushed(&self, event_id: Uuid) -> Result<()> {
        self.db.execute(
            "UPDATE events SET pushed=1 WHERE event_id=?",
            [event_id.to_string()],
        )?;
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
             DELETE FROM sync_outbox;
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
