-- The per-device recovery verifier belonged to a restore feature that no longer
-- exists. Device tokens are chosen by the phone and only their hash is stored.
DROP INDEX devices_recovery_verifier_idx;
ALTER TABLE devices DROP COLUMN recovery_verifier;
