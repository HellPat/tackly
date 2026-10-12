//! The relay's tables in D1. Migrations are applied by the Durable Object
//! before it serves the first request, each as one batch together with its
//! number in `schema_migrations`, so a migration is applied whole or not at
//! all. Append only: a migration that ran is never changed.

use crate::{
    db::{Db, args},
    error::ApiError,
};

/// Each migration: its statements, in order.
const MIGRATIONS: &[&[&str]] = &[&[
    "CREATE TABLE families (
        id TEXT PRIMARY KEY,
        -- The group's current epoch: a commit for it moves it on; messages
        -- for another epoch are refused (the sender catches up).
        epoch INTEGER NOT NULL DEFAULT 0 CHECK (epoch >= 0),
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    )",
    // A device knows its token; the relay keeps only its SHA-256. The head
    // (is_owner) commits and invites; an AI agent (is_agent) only reads, until
    // expires_at; a device the head took out keeps revoked_at.
    "CREATE TABLE devices (
        id TEXT PRIMARY KEY,
        family_id TEXT NOT NULL REFERENCES families(id),
        token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
        is_owner INTEGER NOT NULL DEFAULT 0 CHECK (is_owner IN (0, 1)),
        is_agent INTEGER NOT NULL DEFAULT 0 CHECK (is_agent IN (0, 1)),
        expires_at TEXT,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        revoked_at TEXT
    )",
    "CREATE INDEX devices_family_id_idx ON devices (family_id)",
    // The family's MLS group messages, in order: application messages
    // (events, history snapshots) and commits. The relay reads a message's
    // epoch and kind, nothing else, and keeps every message forever.
    "CREATE TABLE group_messages (
        sequence INTEGER PRIMARY KEY AUTOINCREMENT,
        family_id TEXT NOT NULL REFERENCES families(id),
        event_id TEXT NOT NULL,
        origin_device_id TEXT NOT NULL REFERENCES devices(id),
        epoch INTEGER NOT NULL CHECK (epoch >= 0),
        -- 1: application, 3: commit (MLS content types)
        content_type INTEGER NOT NULL CHECK (content_type IN (1, 3)),
        message BLOB NOT NULL CHECK (length(message) BETWEEN 1 AND 2097152),
        received_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        UNIQUE (family_id, event_id)
    )",
    "CREATE INDEX group_messages_family_sequence_idx ON group_messages (family_id, sequence)",
    "CREATE TRIGGER group_messages_no_update BEFORE UPDATE ON group_messages
     BEGIN SELECT RAISE(ABORT, 'group_messages is append-only'); END",
    "CREATE TRIGGER group_messages_no_delete BEFORE DELETE ON group_messages
     BEGIN SELECT RAISE(ABORT, 'group_messages is append-only'); END",
    // A single-use invitation: open, then pending (a phone asked, with its key
    // package), approved (the head left the welcome), used.
    "CREATE TABLE invitations (
        id TEXT PRIMARY KEY,
        family_id TEXT NOT NULL REFERENCES families(id),
        verifier_hash BLOB NOT NULL CHECK (length(verifier_hash) = 32),
        created_by TEXT NOT NULL REFERENCES devices(id),
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        expires_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+5 minutes')),
        status TEXT NOT NULL DEFAULT 'open'
            CHECK (status IN ('open', 'pending', 'approved', 'used', 'cancelled')),
        pending_device_id TEXT,
        key_package BLOB,
        welcome BLOB,
        joined_after INTEGER
    )",
    "CREATE INDEX invitations_family_id_idx ON invitations (family_id)",
    // The head's sealed recovery backup, under an ID derived from the code.
    "CREATE TABLE recovery_blobs (
        id TEXT PRIMARY KEY,
        family_id TEXT NOT NULL REFERENCES families(id),
        blob BLOB NOT NULL CHECK (length(blob) BETWEEN 1 AND 16777216),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    )",
    "CREATE INDEX recovery_blobs_family_id_idx ON recovery_blobs (family_id)",
    // An AI agent asks for access; the head allows it for a time or denies it.
    "CREATE TABLE agent_requests (
        id TEXT PRIMARY KEY,
        family_id TEXT NOT NULL REFERENCES families(id),
        device_id TEXT NOT NULL,
        key_package BLOB NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending'
            CHECK (status IN ('pending', 'approved', 'denied', 'used')),
        agent_expires_at TEXT,
        welcome BLOB,
        joined_after INTEGER,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        expires_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+10 minutes'))
    )",
    "CREATE INDEX agent_requests_family_idx ON agent_requests (family_id, status)",
]];

#[derive(serde::Deserialize)]
struct Applied {
    version: Option<i64>,
}

/// Applies the migrations D1 does not have yet.
pub async fn migrate(db: &Db) -> Result<(), ApiError> {
    db.run(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
        args![],
    )
    .await?;
    let applied = db
        .first::<Applied>(
            "SELECT MAX(version) AS version FROM schema_migrations",
            args![],
        )
        .await?
        .and_then(|row| row.version)
        .unwrap_or(0);
    for (index, statements) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= applied {
            continue;
        }
        let mut batch = statements
            .iter()
            .map(|sql| db.statement(sql, args![]))
            .collect::<Result<Vec<_>, _>>()?;
        batch.push(db.statement(
            "INSERT INTO schema_migrations (version) VALUES (?)",
            args![version],
        )?);
        db.batch(batch).await?;
    }
    Ok(())
}
