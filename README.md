# Tackly

ADHD optimized task management. Shared family tasks that work offline and sync live.

## What it does

- One task list for the whole family.
- Tap *Start*, tap *Finish*. Tackly keeps who, how long, a note and the place.
- Places: group shops (Grocery Store > LIDL, Aldi), give each a location, and
  see how much there is to get at each one. Add a task from inside a place and
  it is already assigned there.
- Others see changes live.
- Works offline. Syncs when back online.
- Join with a QR code or a link. Both phones show six digits; the head confirms.
- Two people finish the same task? Tackly keeps the better entry. If they
  disagree, only those two decide.
- Encrypted on the phone. The server cannot read your tasks.

Android first, iOS maybe later. No restore: lost phone means join again.

---

# Development

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

### Try it

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

### How it works

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

### Tests

All run against a real server with a SQLite file.

- `just test`: unit tests and device-level end-to-end tests. No windows. CI runs
  it on Linux.
- `just acceptance`: Cucumber scenarios in
  `crates/acceptance/tests/features/`. Three real app windows (Patrick, Mona,
  Mara); typing key by key, clicking, pasting the link, decoding the on-screen
  QR code. Playwright-style API: strict locators, auto-waiting actions,
  retrying `expect`. The engine runs inside the page (`tests/support/driver.js`)
  via the `ui-test` bridge, which exists only in test builds. Windows open and
  close alone; let it finish. CI runs it on macOS.
- `just android-test`: Playwright on an emulator (`android-e2e/spike.mjs`), one
  scenario. Runs next to the desktop suite, not in CI. Limits:
  `AndroidInput.type` drops capitals; attach only after the app has started;
  needs Playwright's driver APK (the recipe installs it).

Server details are in [server/README.md](server/README.md).

### Places and address search

Location autocomplete asks a [Photon](https://photon.komoot.io) server (OpenStreetMap
data); the typed text is sent there and nothing else. Offline, or with no match,
the typed text is kept as the location's name. `TACKLY_GEOCODER` points at
another server; tests use a fake one. Tasks, places and other IDs inside encrypted events are UUIDv7. The four IDs the
server sees (family, device, event, invitation) are random UUIDv4, because a
v7 ID would reveal when something happened.

### Status

Not done, by decision: there is no restore process. A lost phone means
leaving and rejoining with a new invitation. Also missing: device revocation,
key rotation, real GPS on Android, scanning the QR code with the camera, and
keeping secrets in the Android Keystore (they sit in the app-private storage
for now). The Android app builds and runs in the emulator, and its Playwright
spike passes.
