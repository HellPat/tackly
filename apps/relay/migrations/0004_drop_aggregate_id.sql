-- The server does not need to know which task, place or list an event is about:
-- it only relays ciphertext. The routing ID also grouped events by subject.
ALTER TABLE encrypted_events DROP COLUMN aggregate_id;
