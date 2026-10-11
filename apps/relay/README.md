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
DATABASE_URL=sqlite:///var/lib/tackly/tackly-relay.db cargo run --release -p tackly-relay
```

The database file is created if needed. Migrations are embedded and applied on
start; `tackly-relay migrate` applies them and exits. The server binds to
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
| `POST /v1/families/{id}/events` | append up to eight MLS messages (events, snapshots, commits) |
| `GET /v1/families/{id}/events?after=` | one page of messages after a cursor |
| `GET /v1/families/{id}/stream` | live stream (SSE) |
| `POST /v1/families/{id}/invites` | create an invitation (head of the family) |
| `GET`/`DELETE /v1/families/{id}/invites/{invite}` | its status; cancel it |
| `POST /v1/families/{id}/invites/{invite}/approve` | leave the MLS welcome for the new phone |
| `POST /v1/invites/{invite}/request` | a new phone asks to join |
| `POST /v1/invites/{invite}/claim` | the new phone collects the welcome and joins (an agent's token ends at the time given) |
| `DELETE /v1/families/{id}/devices/{device}` | the head takes a device out: its token stops working |
| `POST /v1/families/{id}/token` | a device moves to a new token (after coming back from a backup) |
| `PUT`/`DELETE /v1/families/{id}/recovery/{rid}` | the head leaves its sealed recovery backup; removes it |
| `GET /v1/recovery/{rid}` | the backup, for whoever knows the recovery code (and so the ID) |

The request and response types live in `wire/src/lib.rs`, shared
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
  the same six digits derived from it and the new phone's MLS key package,
  and the head confirms them; the head's phone adds the new phone to the group
  and the server relays the welcome, and where to start reading from.
- **Messages.** Everything in the log is an MLS message to the family's
  group (RFC 9420): application messages (events, and history snapshots for a
  new member) and commits (members added or removed). The server reads a
  message's epoch and kind, nothing else. A message for an epoch that is over
  is refused (`409 stale epoch`): the phone catches up and encrypts again. A
  commit moves the epoch on; only the head's device may commit. Random (v4)
  message IDs make retries idempotent: the same ID from the same device again
  changes nothing. A message must come from the device that sends it. Writes
  use one SQLite connection, so sequence numbers are allocated in commit order
  and a cursor cannot skip a late commit.
- **Roles.** The head's device (`is_owner`) is the only one that commits,
  invites, takes devices out and keeps the recovery backup. A member sends
  events. An AI agent (`is_agent`) only reads, and only until `expires_at`.
  A device the head took out keeps `revoked_at` and is turned away. The
  phones enforce the same rules on the contents; the server's checks are a
  second line.
- **Recovery.** The backup is an opaque blob under an ID the phone derived
  from its recovery code; the server keeps one per ID, lets only the family's
  head replace or remove it, and hands it to anyone who asks by ID (the
  words are the key; the ID says nothing about them).
- **Retention.** Every accepted event is kept forever. There is no expiry,
  pruning or delete API, and database triggers reject updates and deletes of the
  event table (a database administrator can still bypass them). Corrections,
  reversals and reopenings are new events.

The server sees family, device and event IDs (all random, never time-ordered, so
they say nothing about when something happened), payload sizes, upload times, IP
addresses and access patterns. The time of an action is inside the ciphertext.
MLS protects the contents, signs each message with its sender's key, and
numbers messages per sender. The server is never in the group.

## Not done

Protection against a malicious server withholding events (the phones would
not notice a gap in another phone's messages that the server never relayed),
and rate limits. It should not be treated as a complete zero-knowledge
production service until those exist.

## Tests

`just test` runs the end-to-end suite in `phone/tests/e2e.rs`, which
starts this server in-process on a real SQLite file and drives real devices
against it, including the registration retry and server outages.
