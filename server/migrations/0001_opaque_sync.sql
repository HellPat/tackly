CREATE TABLE families (
    id UUID PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE devices (
    id UUID PRIMARY KEY,
    family_id UUID NOT NULL REFERENCES families(id),
    token_hash BYTEA NOT NULL UNIQUE,
    recovery_verifier BYTEA NOT NULL CHECK (octet_length(recovery_verifier) = 32),
    is_owner BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ
);
CREATE INDEX devices_family_id_idx ON devices (family_id);
CREATE INDEX devices_recovery_verifier_idx ON devices (family_id, recovery_verifier);

CREATE TABLE encrypted_events (
    sequence BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    family_id UUID NOT NULL REFERENCES families(id),
    event_id UUID NOT NULL,
    aggregate_id UUID NOT NULL,
    origin_device_id UUID NOT NULL REFERENCES devices(id),
    key_version INTEGER NOT NULL CHECK (key_version > 0),
    nonce BYTEA NOT NULL CHECK (octet_length(nonce) = 12),
    ciphertext BYTEA NOT NULL CHECK (octet_length(ciphertext) BETWEEN 1 AND 2097152),
    mac BYTEA NOT NULL CHECK (octet_length(mac) = 16),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (family_id, event_id)
);
CREATE INDEX encrypted_events_family_sequence_idx
    ON encrypted_events (family_id, sequence);

CREATE TABLE invitations (
    id UUID PRIMARY KEY,
    family_id UUID NOT NULL REFERENCES families(id),
    verifier_hash BYTEA NOT NULL CHECK (octet_length(verifier_hash) = 32),
    created_by UUID NOT NULL REFERENCES devices(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '5 minutes'),
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'pending', 'approved', 'used', 'cancelled')),
    pending_device_id UUID,
    package_nonce BYTEA,
    package_ciphertext BYTEA,
    package_mac BYTEA
);
CREATE INDEX invitations_family_id_idx ON invitations (family_id);
