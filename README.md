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
2. Patrick: *Family → Invite someone*. A **QR code** and a **link** appear.
   *Copy link* puts the link on the clipboard so it can be sent by Signal or
   any messenger. The code expires after five minutes.
3. Mona: *Join with an invitation*, paste the link, *Ask to join*. Both phones
   show the same six digits; Patrick confirms. Repeat for Mara. (A phone scans
   the QR code with its camera; the desktop windows have no scanner, so paste
   the link there. `tackly-app 'tackly://join?c=…'` opens straight on the form.)
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

Two layers, both against a real server with a SQLite file:

**`just acceptance`: Cucumber, real windows.** `crates/acceptance/tests/features/*.feature`
are plain-language scenarios. For each one the suite opens three real app
windows (Patrick, Mona, Mara) and does everything the way a person does:
typing key by key, clicking, pasting the invitation link, even scanning the QR
code (the test draws the on-screen SVG and decodes the pixels, like a camera,
and checks it matches the link shown). The API is Playwright's: lazy, strict
locators (`get_by_role`, `get_by_label`, `filter`), actions that auto-wait for
an element to be visible, enabled, stable and not covered, `press_sequentially`,
`press`, and retrying `expect(...).to_have_text(...)` assertions. The embedded
web view has no Playwright/CDP protocol, so the engine runs inside the page
(`tests/support/driver.js`) and is reached through the app's `ui-test` bridge
(`crates/app/src/uitest.rs`), which exists only in that build, never in
`just start` or release builds. Windows open and close by themselves while it
runs; let it finish. CI runs it on macOS.

| Feature | Scenarios |
| --- | --- |
| `family.feature` | create a family and connect two members (one by QR code, one by link); the task list syncs, others watch a start live, finishing records duration, note and location; someone else finishes a started task and the head reopens it |
| `offline.feature` | the whole flow with no server, shared later; a server outage while three members keep working, then everyone catches up |
| `typing.feature` | the Add button follows each typed key, Backspace and Enter work |
| `conflict.feature` | two finish the same task, only they may decide, the winner is shown everywhere |

**`just test`: no windows.** Unit tests plus `crates/client/tests/e2e.rs`,
which runs the same flows through the device core directly. It is fast and is
what CI runs on Linux.

Server details are in [server/README.md](server/README.md).

## Status

Not done, by decision: there is no restore process. A lost phone means
leaving and rejoining with a new invitation. Also missing: device revocation,
key rotation, real GPS on Android; the Android build is unverified.
