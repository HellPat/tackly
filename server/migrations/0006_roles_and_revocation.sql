-- Who may do what on the relay: the head commits; members send events; an
-- AI agent only reads, and only until its time is over. A device the head
-- took out of the family keeps revoked_at and gets nothing.
ALTER TABLE devices ADD COLUMN is_agent INTEGER NOT NULL DEFAULT 0 CHECK (is_agent IN (0, 1));
ALTER TABLE devices ADD COLUMN expires_at TEXT;
-- An invitation for an AI agent says how long the agent may read.
ALTER TABLE invitations ADD COLUMN agent_seconds INTEGER CHECK (agent_seconds IS NULL OR agent_seconds > 0);
