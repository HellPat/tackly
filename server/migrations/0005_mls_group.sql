-- The family is an MLS group now (RFC 9420). The relay stores the group's
-- messages in order: application messages (events, history snapshots) and
-- commits (members added or removed), which move the group to a new epoch.
-- The relay reads a message's epoch and kind, nothing else. No deployment
-- holds data in the old table yet.
DROP TABLE encrypted_events;

CREATE TABLE group_messages (
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
);
CREATE INDEX group_messages_family_sequence_idx ON group_messages (family_id, sequence);

-- Accepted messages are retained forever.
CREATE TRIGGER group_messages_no_update
BEFORE UPDATE ON group_messages
BEGIN
    SELECT RAISE(ABORT, 'group_messages is append-only');
END;
CREATE TRIGGER group_messages_no_delete
BEFORE DELETE ON group_messages
BEGIN
    SELECT RAISE(ABORT, 'group_messages is append-only');
END;

-- The group's current epoch: a commit for it moves it on; messages for an
-- older epoch are refused, so the sender catches up and encrypts again.
ALTER TABLE families ADD COLUMN epoch INTEGER NOT NULL DEFAULT 0;

-- An invitation carries the joiner's key package in, and the welcome (and
-- where to start reading) out. The old sealed-key columns stay unused.
ALTER TABLE invitations ADD COLUMN key_package BLOB;
ALTER TABLE invitations ADD COLUMN welcome BLOB;
ALTER TABLE invitations ADD COLUMN joined_after INTEGER;
