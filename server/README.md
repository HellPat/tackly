# Tackly sync server

A headless relay for the app's append-only events, on SQLite. It keeps each
family's events as ciphertext, hands them to the family's phones by cursor, and
pushes new ones over Server-Sent Events. It cannot read task names, notes or
coordinates: every phone decrypts and replays the events itself.

```
src/events.rs    the encrypted event log: append, read by cursor, live stream
src/families.rs  registering a family
src/invites.rs   letting a new phone join
src/auth.rs      who is calling (a device's bearer token)
src/db.rs        opening the database, reading columns without panicking
migrations/      the schema; applied when the server starts
```

## Run

```sh
DATABASE_URL=sqlite:///var/lib/tackly/tackly-sync.db cargo run --release -p tackly-sync
```

The database file is created if needed. Migrations are embedded and applied on
start; `tackly-sync migrate` applies them and exits. The server binds to
`127.0.0.1:3000` (`TACKLY_BIND` changes it). `/health` answers 204 and exposes
nothing.

Put a TLS reverse proxy in front of it and give the app the proxy's HTTPS URL.
Do not expose the listener itself to the internet without TLS, request limits
and abuse controls. This repository does not deploy the server or back up its
database; the file and its write-ahead log both need backing up.

## API

| | |
| --- | --- |
| `POST /v1/families` | register a family and its first device |
| `POST /v1/families/{id}/events` | append up to eight encrypted events |
| `GET /v1/families/{id}/events?after=` | one page of events after a cursor |
| `GET /v1/families/{id}/stream` | live stream (SSE) |
| `POST /v1/families/{id}/invites` | create an invitation (head of the family) |
| `GET`/`DELETE /v1/families/{id}/invites/{invite}` | its status; cancel it |
| `POST /v1/families/{id}/invites/{invite}/approve` | hand over the sealed family key |
| `POST /v1/invites/{invite}/request` | a new phone asks to join |
| `POST /v1/invites/{invite}/claim` | the new phone collects the key and joins |

The request and response types live in `crates/protocol/src/wire.rs`, shared
with the app.

**Live stream.** `GET /v1/families/{id}/stream?after=<sequence>` replays the
events after the cursor, then pushes each newly accepted batch as an `events`
message whose data is a page of events and whose SSE `id` is the last sequence.
A reconnecting client sends it back as `Last-Event-ID`. Keep-alives come every
15 seconds.

## Design

- **Device tokens are chosen by the phone.** A phone makes a random token,
  keeps it, and sends only its SHA-256 hash when it registers or joins. The
  server never sees a token it could reveal, and repeating a request whose
  answer was lost is harmless: same device, same hash, same answer. A different
  token for an existing family is a conflict.
- **Invitations.** The head of the family creates a single-use invitation that
  expires after five minutes. Its secret is shown as a QR code and a link and is
  never uploaded. The new phone proves it knows the secret; both phones show
  the same six digits derived from it and the head confirms them; the head's
  phone seals the family key under the secret and the server relays that
  package.
- **Events.** UUID event IDs make retries idempotent: the same bytes again are
  accepted, other bytes under the same ID are a conflict. An event must come
  from the device that sends it. Writes use one SQLite connection, so sequence
  numbers are allocated in commit order and a cursor cannot skip a late commit.
- **Retention.** Every accepted event is kept forever. There is no expiry,
  pruning or delete API, and database triggers reject updates and deletes of the
  event table (a database administrator can still bypass them). Corrections,
  reversals and reopenings are new events.

The server sees family, device and event IDs, payload sizes, upload times, IP
addresses and access patterns. The time of an action is inside the ciphertext.
AES-256-GCM protects the contents and authenticates the routing IDs as
associated data. The server does not have the family key.

## Not done

Device revocation, family-key rotation (old events are never re-encrypted, so
old keys must stay available to clients), signed client event histories, and
protection against a malicious server withholding events. It should not be
treated as a complete zero-knowledge production service until those exist.

## Tests

`just test` runs the end-to-end suite in `crates/client/tests/e2e.rs`, which
starts this server in-process on a real SQLite file and drives real devices
against it, including the registration retry and server outages.
