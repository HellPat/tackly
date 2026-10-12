-- Each start of the relay. It may have come back from a backup that misses
-- the messages it accepted last (Litestream replicates within a second): those
-- after `after` may be lost. Sequence numbers go on from `sequence`, far past
-- any number used before, so a number is never given twice and phones that
-- read further than the backup do not skip what comes next. Phones send their
-- own messages after `after` again (the IDs make that harmless).
CREATE TABLE restarts (
    sequence INTEGER PRIMARY KEY,
    after INTEGER NOT NULL CHECK (after >= 0 AND after < sequence),
    started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
