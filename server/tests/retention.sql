-- Run only against a disposable test database.
DO $$
DECLARE
    family_uuid uuid := gen_random_uuid();
    device_uuid uuid := gen_random_uuid();
    event_uuid uuid := gen_random_uuid();
    blocked boolean;
BEGIN
    INSERT INTO families(id) VALUES (family_uuid);
    INSERT INTO devices(id, family_id, token_hash, recovery_verifier)
    VALUES (device_uuid, family_uuid,
            decode(replace(gen_random_uuid()::text, '-', '') ||
                   replace(gen_random_uuid()::text, '-', ''), 'hex'),
            decode(replace(gen_random_uuid()::text, '-', '') ||
                   replace(gen_random_uuid()::text, '-', ''), 'hex'));
    INSERT INTO encrypted_events(
        family_id, event_id, aggregate_id, origin_device_id,
        key_version, nonce, ciphertext, mac
    ) VALUES (
        family_uuid, event_uuid, gen_random_uuid(), device_uuid,
        1, decode(repeat('33', 12), 'hex'),
        decode('44', 'hex'), decode(repeat('55', 16), 'hex')
    );

    blocked := false;
    BEGIN
        UPDATE encrypted_events SET key_version = 2 WHERE event_id = event_uuid;
    EXCEPTION WHEN raise_exception THEN
        IF SQLERRM = 'encrypted_events is append-only' THEN
            blocked := true;
        ELSE
            RAISE;
        END IF;
    END;
    IF NOT blocked THEN RAISE EXCEPTION 'UPDATE was allowed'; END IF;

    blocked := false;
    BEGIN
        DELETE FROM encrypted_events WHERE event_id = event_uuid;
    EXCEPTION WHEN raise_exception THEN
        IF SQLERRM = 'encrypted_events is append-only' THEN
            blocked := true;
        ELSE
            RAISE;
        END IF;
    END;
    IF NOT blocked THEN RAISE EXCEPTION 'DELETE was allowed'; END IF;

    blocked := false;
    BEGIN
        TRUNCATE encrypted_events;
    EXCEPTION WHEN raise_exception THEN
        IF SQLERRM = 'encrypted_events is append-only' THEN
            blocked := true;
        ELSE
            RAISE;
        END IF;
    END;
    IF NOT blocked THEN RAISE EXCEPTION 'TRUNCATE was allowed'; END IF;

    IF (SELECT count(*) FROM encrypted_events WHERE event_id = event_uuid) != 1 THEN
        RAISE EXCEPTION 'event was not retained';
    END IF;
END;
$$;
