-- An AI agent asks a family for access: it names the family, makes its key,
-- and waits. The head's phone sees the request, compares the code both show
-- and allows it for a time (or denies it). Agents no longer come by invitation.
CREATE TABLE agent_requests (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES families(id),
    device_id TEXT NOT NULL,
    key_package BLOB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'approved', 'denied', 'used')),
    -- Set when the head allows it: when the agent's token ends.
    agent_expires_at TEXT,
    welcome BLOB,
    joined_after INTEGER,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    expires_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+10 minutes'))
);
CREATE INDEX agent_requests_family_idx ON agent_requests (family_id, status);

-- Invitations are for phones only again.
ALTER TABLE invitations DROP COLUMN agent_seconds;
