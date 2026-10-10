# Tackly

Shared family tasks that work offline and sync live. Written in Rust: a
[Dioxus](https://dioxuslabs.com) app with an Android-style (Material) UI, an
Axum sync relay, SQLite on both sides, and a Nix dev shell.

```
crates/protocol   shared by app and server: the Family aggregate, its commands
                  and events (cqrs-es), and the HTTP/SSE wire types
crates/client     the device core: encrypted SQLite event log, pairing, sync
crates/app        the Dioxus app
server            the relay: stores encrypted events, pushes them over SSE
```

## Try it

Install [Nix](https://nixos.org/download) (flakes enabled), then:

```sh
just start
```

This builds everything, starts the relay and opens **three app windows**
(Patrick, Mona, Mara; Patrick is the head of the family), each with its own database under `.dev/`.

1. In Patrick's window: *Create a family*.
2. Patrick: *Family → Invite someone*, copy the code.
3. Mona: *Join with an invitation*, paste the code, *Ask to join*. Both phones
   show the same six digits; Patrick confirms. Repeat for Mara.
4. Add tasks, tap *Start* or *Finish* (optional note and location) and watch the
   other windows update live.

`just start 1` opens one window; `just reset` forgets all test data;
`just test` runs the suites; `just server` runs only the relay. The desktop
build fakes the GPS with `TACKLY_LOCATION` (each window gets its own).
`just android` runs the same app in an emulator (Android SDK/NDK required; not
covered by CI).

Without Nix, install Rust and run `cargo run -p tackly-sync` and
`cargo run -p tackly-app` yourself.

## How it works

- **CQRS / event sourcing** with [`cqrs-es`](https://doc.rust-cqrs.org). The
  family is one aggregate (`crates/protocol/src/aggregate.rs`): commands such as
  `StartTask` or `CompleteTask` are validated against the replayed state and
  produce `FamilyEvent`s. Completion events carry metadata: who, when, how
  long since the start, an optional note and location.
- **Offline first.** Every command commits to the phone's SQLite log first.
  A family can be created and used with no server; the phone registers and
  uploads later. Pairing needs the server.
- **Live updates.** The app keeps an SSE connection
  (`GET /v1/families/{id}/stream`) and reconnects from its cursor with backoff.
- **Conflicts.** If two members finish the same task while apart, both
  completions are kept and shown. Either of them can pick the winner; others
  cannot.
- **Encrypted.** Events are encrypted on the phone with a family key
  (AES-256-GCM). The server stores ciphertext and routing IDs only. The family
  key travels to a new member sealed under the invitation secret.

## Tests

`crates/client/tests/e2e.rs` runs a real server (SQLite file, real HTTP/SSE)
and real devices:

| Test | Covers |
| --- | --- |
| `three_members_share_tasks_and_see_each_other_live` | create family, two members join with code confirmation, task list sync, start/finish with metadata, reopen, identical state on all three |
| `double_completion_is_settled_by_a_member_who_took_part` | concurrent finish, conflict visible everywhere, outsider refused, participant resolves |
| `everything_works_without_a_server_and_syncs_later` | family and tasks created with the server down, registers and uploads later, a new member receives the history |
| `a_server_outage_does_not_stop_members_and_they_converge_afterwards` | server killed mid-session, all three keep working, converge after restart |

Server details are in [server/README.md](server/README.md).

## Status

Not done, by decision: there is no restore process. A lost phone means
leaving and rejoining with a new invitation. Also missing: device revocation,
key rotation, real GPS on Android; the Android build is unverified.
