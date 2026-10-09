PRAGMA foreign_keys = ON;

CREATE TABLE families (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES families(id),
    token_hash BLOB NOT NULL UNIQUE,
    recovery_verifier BLOB NOT NULL CHECK (length(recovery_verifier) = 32),
    is_owner INTEGER NOT NULL DEFAULT 0 CHECK (is_owner IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    revoked_at TEXT
);
CREATE INDEX devices_family_id_idx ON devices (family_id);
CREATE INDEX devices_recovery_verifier_idx ON devices (family_id, recovery_verifier);

CREATE TABLE encrypted_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    family_id TEXT NOT NULL REFERENCES families(id),
    event_id TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    origin_device_id TEXT NOT NULL REFERENCES devices(id),
    key_version INTEGER NOT NULL CHECK (key_version > 0),
    nonce BLOB NOT NULL CHECK (length(nonce) = 12),
    ciphertext BLOB NOT NULL CHECK (length(ciphertext) BETWEEN 1 AND 2097152),
    mac BLOB NOT NULL CHECK (length(mac) = 16),
    received_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (family_id, event_id)
);
CREATE INDEX encrypted_events_family_sequence_idx
    ON encrypted_events (family_id, sequence);

CREATE TABLE invitations (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES families(id),
    verifier_hash BLOB NOT NULL CHECK (length(verifier_hash) = 32),
    created_by TEXT NOT NULL REFERENCES devices(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+5 minutes')),
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'pending', 'approved', 'used', 'cancelled')),
    pending_device_id TEXT,
    package_nonce BLOB,
    package_ciphertext BLOB,
    package_mac BLOB
);
CREATE INDEX invitations_family_id_idx ON invitations (family_id);
