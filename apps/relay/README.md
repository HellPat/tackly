# Tackly relay

A headless relay for the app's encrypted messages, as a Cloudflare Worker. It
keeps each family's MLS messages as ciphertext, hands them to the family's
phones by cursor, and pushes new ones over WebSockets. It cannot read task
names, notes or coordinates: every phone decrypts and replays the messages
itself.

```
src/lib.rs        the Worker: every request goes to one Durable Object
src/relay.rs      the Durable Object: one request at a time, routing
src/live.rs       live WebSockets (hibernatable), each remembering its cursor
src/events.rs     the encrypted log: append, read by cursor
src/families.rs   registering a family
src/invites.rs    letting a new phone join
src/agents.rs     an AI agent asks for access, the head allows it
src/recovery.rs   the head's sealed backup
src/auth.rs       who is calling (a device's bearer token), removal, new token
src/db.rs         D1: statements, rows, batches
src/schema.rs     the tables, as migrations applied on the first request
```

## Run

`just server` runs it locally under `wrangler dev` (the Worker, its Durable
Object and a local D1) on `127.0.0.1:3000`, with its data in `.dev/relay/`.
`just test` starts one such relay per end-to-end test; `just test-deployed`
runs the suite against `https://relay.tackly.hellpat.com`.

`build.sh` builds the Worker with `worker-build` (the Nix shell has it, and
the WebAssembly target). On Cloudflare's build machine it installs Rust and
`worker-build` first.

## Deploy

CI deploys from `main` after the tests pass: `scripts/deploy-relay.sh` calls
the Workers Builds deploy hook (Cloudflare pulls the repository and runs
`npm run deploy`, which names the commit in `TACKLY_COMMIT`), then waits up to
twenty minutes for `/health` to answer 204 with that commit in
`x-tackly-commit`, and fails the pipeline otherwise. Pushes alone do not
deploy. D1 (`tackly-relay`) and the Durable Object sit in Western Europe.

## API

| | |
| --- | --- |
| `POST /v1/families` | register a family and its first device |
| `POST /v1/families/{id}/events` | append up to eight MLS messages (events, snapshots, commits) |
| `GET /v1/families/{id}/events?after=` | one page of messages after a cursor |
| `GET /v1/families/{id}/stream?after=` | live WebSocket |
| `POST /v1/families/{id}/invites` | create an invitation (head of the family) |
| `GET`/`DELETE /v1/families/{id}/invites/{invite}` | its status; cancel it |
| `POST /v1/families/{id}/invites/{invite}/approve` | leave the MLS welcome for the new phone |
| `POST /v1/invites/{invite}/request` | a new phone asks to join |
| `POST /v1/invites/{invite}/claim` | the new phone collects the welcome and joins |
| `POST`/`GET /v1/families/{id}/agent-requests` | an AI agent asks for access (no token; at most five wait); the head lists what waits |
| `POST /v1/families/{id}/agent-requests/{request}/approve`, `…/deny` | the head allows the agent for a time and leaves the welcome, or denies it |
| `POST /v1/agent-requests/{request}/claim` | the agent collects its access; its token ends at the time given |
| `DELETE /v1/families/{id}/devices/{device}` | the head takes a device out: its token stops working |
| `POST /v1/families/{id}/token` | a device moves to a new token (after coming back from a backup) |
| `PUT`/`DELETE /v1/families/{id}/recovery/{rid}` | the head leaves its sealed recovery backup; removes it |
| `GET /v1/recovery/{rid}` | the backup, for whoever knows the recovery code (and so the ID) |

The request and response types live in `wire/src/lib.rs`, shared with the
app. Errors are `{"error": "..."}` with the status code.

**Live.** `GET /v1/families/{id}/stream?after=<sequence>` with
`Upgrade: websocket` and the device token. The relay sends every message
after the cursor, then each newly stored batch, as an `EventsPage` in JSON
text. The phone sends `ping` every 30 seconds; the runtime answers `pong`
without waking the Durable Object, which may sleep while sockets stay open.
After a drop the phone reconnects from its cursor.

## Design

- **One Durable Object.** It exists once, so it is where the live sockets
  meet the writes that should wake them, and it runs one request at a time:
  a handler reads D1, decides, and writes one batch (D1 runs a batch as one
  transaction; it has no transactions across calls). Sequence numbers are
  therefore given in order.
- **Device tokens are chosen by the phone.** A phone makes a random token,
  keeps it, and sends only its SHA-256 hash when it registers or joins.
  Repeating a request whose answer was lost is harmless: same device, same
  hash, same answer. A different token for an existing family is a conflict.
- **Invitations.** The head creates a single-use invitation that expires after
  five minutes. Its secret is shown as a QR code and a link and never
  uploaded. The new phone proves it knows the secret; both phones show the
  same six digits derived from it and the new phone's MLS key package, and the
  head confirms them; the head's phone adds the new phone to the group and the
  relay passes on the welcome, and where to start reading from.
- **Messages.** Everything in the log is an MLS message to the family's group
  (RFC 9420): application messages (events, and history snapshots for a new
  member) and commits (members added or removed). The relay reads a message's
  epoch and kind (with openmls), nothing else. A message for another epoch is
  refused (`409 stale epoch`): the phone catches up and encrypts again. A
  commit moves the epoch on; only the head's device may commit. Random (v4)
  message IDs make retries idempotent. A message must come from the device
  that sends it.
- **Roles.** The head's device (`is_owner`) commits, invites, takes devices
  out and keeps the recovery backup. A member sends events. An AI agent
  (`is_agent`) only reads, and only until `expires_at`. A device the head took
  out keeps `revoked_at` and is turned away.
- **Recovery.** The backup is an opaque blob under an ID the phone derived
  from its recovery code; one per ID; only the family's head replaces or
  removes it; anyone who asks by ID gets it.
- **Retention.** Every accepted message is kept forever; triggers reject
  updates and deletes of the log. Corrections are new events.

The relay sees family, device and message IDs (random), payload sizes, upload
times, IP addresses and access patterns. MLS protects the contents, signs each
message with its sender's key, and numbers messages per sender.

## Not done

Protection against a relay withholding messages (phones would not notice a
gap in another phone's messages), and rate limits.
