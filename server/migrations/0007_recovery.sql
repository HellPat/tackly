-- The head's recovery backup: sealed on the phone, kept here under an ID
-- derived from the recovery code. The relay cannot open it; whoever knows
-- the code can fetch it. Only the family's head replaces or removes it.
CREATE TABLE recovery_blobs (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES families(id),
    blob BLOB NOT NULL CHECK (length(blob) BETWEEN 1 AND 16777216),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX recovery_blobs_family_id_idx ON recovery_blobs (family_id);
