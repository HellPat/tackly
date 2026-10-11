-- Accepted ciphertext is retained forever. SQLite has no TRUNCATE statement.
CREATE TRIGGER encrypted_events_no_update
BEFORE UPDATE ON encrypted_events
BEGIN
    SELECT RAISE(ABORT, 'encrypted_events is append-only');
END;

CREATE TRIGGER encrypted_events_no_delete
BEFORE DELETE ON encrypted_events
BEGIN
    SELECT RAISE(ABORT, 'encrypted_events is append-only');
END;
