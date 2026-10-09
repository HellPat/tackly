-- Retain every accepted encrypted event. Corrections are new events, never
-- updates to or deletion of an existing envelope.
CREATE FUNCTION reject_encrypted_event_mutation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'encrypted_events is append-only';
    RETURN NULL;
END;
$$;

CREATE TRIGGER encrypted_events_no_update_or_delete
BEFORE UPDATE OR DELETE ON encrypted_events
FOR EACH ROW EXECUTE FUNCTION reject_encrypted_event_mutation();

CREATE TRIGGER encrypted_events_no_truncate
BEFORE TRUNCATE ON encrypted_events
FOR EACH STATEMENT EXECUTE FUNCTION reject_encrypted_event_mutation();
