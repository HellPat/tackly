# Tackly sync server

This is a headless SQLite relay for the Android app's append-only events.
It stores encrypted event bodies and encrypted invitation key packages. It
cannot project task lists or read task names, photos, completion details, or
coordinates. Each phone decrypts and replays its own events.

## Run

Choose a private, writable location for the SQLite database and apply the
migration explicitly. The database file is created if needed:

```sh
cd server
DATABASE_URL=sqlite:///var/lib/tackly/tackly-sync.db cargo run -- migrate
DATABASE_URL=sqlite:///var/lib/tackly/tackly-sync.db cargo run --release
```

The server binds to `127.0.0.1:3000` by default. Set `TACKLY_BIND` to another
socket address if needed. Put a TLS reverse proxy in front of it and enter the
proxy's HTTPS URL in the app. The app accepts plain HTTP only for debug builds
addressing an Android emulator's local development host. Do not expose the Rust
listener directly to the internet without TLS, request limits, and abuse
controls. `/health` returns 204 and does not expose data.

The server and database are not deployed by this repository. A public HTTPS
endpoint must be supplied before two phones can
pair or synchronize outside a local development network.

With a disposable local database and the server running, run
`python3 tests/transport_smoke.py` and `python3 tests/api_e2e.py`.
The latter covers every current HTTP endpoint, authorization and tenant
isolation, event validation/paging/idempotency, concurrent writes, one-use
joining, invitation expiry, cancellation, and owner/member recovery. Set
`TACKLY_TEST_DB_PATH=/path/to/test.db` to include the expiry test
against the disposable database. Run
`python3 tests/retention.py` with the same variable only against a disposable
database to verify the append-only triggers. These API tests use
synthetic payload bytes; the Android live-relay suite covers actual client
encryption against this server. Neither is a two-physical-phone test.

## Protocol

- A phone can create one family locally without a server. Once a server is
  configured, it registers that family and receives a random device bearer token. The
  server stores only its SHA-256 hash. Each phone generates its own recovery
  secret; the server stores only a verifier for that device's role. The full
  recovery code also includes the family data key and stays on the phone.
- An owner creates a single-use invitation that expires after five minutes.
  The QR contains a random secret that is never uploaded. The joining phone
  requests access, and the owner confirms a matching code on both phones. The owner's phone
  encrypts the family key under the QR secret; the server relays that package.
- Phones POST encrypted events to `/v1/families/{id}/events` and fetch them by
  cursor from the same path. UUID event IDs make retries idempotent. Server
  writes are serialized per family so a cursor cannot skip a late commit.
- The server retains every accepted encrypted event indefinitely. It has no
  event expiry, pruning, or deletion API. Database triggers reject updates and
  deletes of the event table. Corrections, reversals, and reopenings append
  new events; projections and snapshots are rebuildable caches, not replacements
  for the event history.
- The app keeps an encrypted SQLite event log, a durable encrypted upload
  envelope, and a cursor. It accepts edits while offline, retries when opened
  or resumed, polls while active, and asks Android WorkManager for a
  network-constrained retry. Android decides the actual background timing;
  fifteen minutes is its minimum periodic interval, not a delivery guarantee.
- Competing edits on the same task are held for a choice on the phones; other
  tasks remain usable. Task state is replayed from events, not stored as
  mutable server rows.

The server sees family, device, aggregate and event UUIDs, payload sizes,
upload times, IP addresses, and access patterns. The original action time is
inside the ciphertext. AES-256-GCM protects event contents and authenticates
the routing IDs as associated data.
The server does not have the family key. Someone holding an owner's full
recovery code can recover an owner device and decrypt family data; a member's
code recovers only member access. Store these codes in a password manager and
do not put them in server configuration or logs.

Indefinite retention also needs durable SQLite file backups that include the
write-ahead log, plus periodic restore checks; this repository does not deploy
or operate those backups. A privileged database administrator can bypass the
trigger. Future key rotation must keep
the old decryption keys available to authorized clients, because old event
ciphertext is never rewritten. Future event-schema upcasters should run during
client replay and leave stored events intact.

The current sharing slice does **not** provide device revocation, family-key
rotation, signed client event histories, protection from a malicious server
withholding events, or a resilient retry if the join response is lost after a
one-use invitation is consumed. It should not be treated as a complete
zero-knowledge production service until those controls are implemented and the
two-device offline/reconnect flow has been exercised on real Android builds.

## Future MCP access

A bearer API token can authorize event reads and writes but cannot decrypt the
family. A future MCP that handles plaintext must run in a trusted client
environment with the family key as well as an API token. The server must not
receive that key. No MCP endpoint or token-management UI is implemented yet.
