# Tackly

Shared family tasks that work offline and sync live. Written in Rust: a
[Dioxus](https://dioxuslabs.com) app with an Android-style (Material) UI, an
Axum sync relay, SQLite on both sides, and a Nix dev shell.

```
crates/protocol    shared by app and server: the Family aggregate, its commands
                   and events (cqrs-es), and the HTTP/SSE wire types
crates/client      the device core: encrypted SQLite event log, pairing, sync
crates/app         the Dioxus app (desktop windows and Android)
server             the relay: stores encrypted events, pushes them over SSE
crates/testkit     a stoppable real relay, shared by the test suites
crates/acceptance  Cucumber scenarios clicked through real app windows
android-e2e        Playwright on an Android emulator (a spike, see below)
scripts            what the just recipes run
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
`just server` runs only the relay. The desktop build fakes the GPS with
`TACKLY_LOCATION` (each window gets its own).

| Recipe | |
| --- | --- |
| `just test` | unit tests and the device-level end-to-end suite |
| `just acceptance` | the Cucumber suite in real windows |
| `just lint` | clippy on everything |
| `just android-build` | debug APK (Android SDK and NDK needed; the Nix shell has the rest) |
| `just android` | build, install and start it on the emulator or device, booting the first AVD if none runs |
| `just android-test` | the Playwright-on-Android suite |

Without Nix, install Rust (with the clippy component), and for Android the SDK,
NDK, JDK 21 and `dioxus-cli`, then run the scripts in `scripts/` or
`cargo run -p tackly-sync` and `cargo run -p tackly-app` yourself.

**No unwrap.** Errors are handled, not unwrapped: `unwrap` and `expect` are
denied by clippy (tests may use them). `.cargo/config.toml` runs clippy on every
workspace build, so a stray `unwrap()` fails plain `cargo build`, not only
`cargo clippy`.

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
- **Conflicts.** If two members finish the same task while apart and one
  completion already contains everything the other recorded (identical, or the
  other has no note, duration or location), it simply wins and nobody is asked.
  If they disagree, both are kept and shown; either of them can pick the
  winner, others cannot.
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

**`just android-test`: Playwright on Android (spike).** `android-e2e/spike.mjs`
drives the app in an emulator: Playwright's `Page` on the app's WebView for
locators and assertions, and `AndroidInput` over adb for real touches and real
keyboard input. It runs one scenario (create a family, add a task by keyboard
and Enter, start and finish it) against a real relay on the host. It is an
alternative next to the desktop suite, not a replacement, and is not in CI.
Known limits: `AndroidInput.type` drops capital letters, attaching while the app
is still starting crashes the debug build (the script waits a moment first), and
Playwright's Android driver APK must be installed (`npx playwright install
android`, done by the recipe).

Server details are in [server/README.md](server/README.md).

## Status

Not done, by decision: there is no restore process. A lost phone means
leaving and rejoining with a new invitation. Also missing: device revocation,
key rotation, real GPS on Android, scanning the QR code with the camera, and
keeping secrets in the Android Keystore (they sit in the app-private storage
for now). The Android app builds and runs in the emulator, and its Playwright
spike passes.
