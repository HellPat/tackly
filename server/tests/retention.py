"""Check immutable ciphertext rows in a disposable SQLite relay database."""

import os
import secrets
import sqlite3
import uuid

path = os.environ["TACKLY_TEST_DB_PATH"]
identifier = lambda: str(uuid.uuid4())
family_id, device_id, event_id = identifier(), identifier(), identifier()

with sqlite3.connect(path) as db:
    db.execute("PRAGMA foreign_keys=ON")
    db.execute("INSERT INTO families(id) VALUES (?)", (family_id,))
    db.execute(
        "INSERT INTO devices(id,family_id,token_hash,recovery_verifier) VALUES (?,?,?,?)",
        (device_id, family_id, secrets.token_bytes(32), secrets.token_bytes(32)),
    )
    db.execute(
        """INSERT INTO encrypted_events(
           family_id,event_id,aggregate_id,origin_device_id,key_version,nonce,ciphertext,mac
           ) VALUES (?,?,?,?,?,?,?,?)""",
        (family_id, event_id, identifier(), device_id, 1,
         secrets.token_bytes(12), b"event", secrets.token_bytes(16)),
    )
    for statement in (
        "UPDATE encrypted_events SET key_version=2 WHERE event_id=?",
        "DELETE FROM encrypted_events WHERE event_id=?",
    ):
        try:
            db.execute(statement, (event_id,))
        except sqlite3.IntegrityError as error:
            assert "encrypted_events is append-only" in str(error), error
        else:
            raise AssertionError(f"Mutation was allowed: {statement}")
    count = db.execute(
        "SELECT count(*) FROM encrypted_events WHERE event_id=?", (event_id,)
    ).fetchone()[0]
    assert count == 1, count
print("append-only retention passed")
