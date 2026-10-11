# Tackly

ADHD optimized task management. Shared family tasks that work offline and sync live.

## What it does

- Tasks for the whole family. New ones land in *Other*; make lists when you want them.
- Tick a task off and it is gone. Changed your mind? *Undo*.
- *Mine*, *Unassigned*, *All*: see only what is yours, or pick something free.
- Take a task, or give it to someone. Tasks others have are greyed out.
- Open a task to focus: a big clock, *Start*, *Pause*, *Finish*.
- Places: shops in groups (Grocery Store > LIDL, Aldi), each with its
  locations, and how much there is to get at each one.
- Others see changes live. Works offline; syncs when back online.
- Join with a QR code or a link. Both phones show six digits; the head confirms.
- Your name, your picture (an icon or a photo), your color scheme.
- Notes where a task was finished (never shown; Android asks once).
- Encrypted on the phone. The server cannot read your tasks.

Android first, iOS maybe later. No restore: lost phone means join again.

---

# Development

```
crates/protocol    shared by app and server: the Family aggregate, its commands
                   and events (cqrs-es), and the HTTP/SSE wire types
crates/client      the device core: encrypted SQLite event log, pairing, sync
crates/app         the Dioxus app (desktop windows and Android), styled with Tailwind
server             the relay: stores encrypted events, pushes them over SSE
crates/testkit     a stoppable real relay, shared by the test suites
crates/acceptance  Cucumber scenarios clicked through real app windows
android-e2e        Playwright on an Android emulator (see below)
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
2. Patrick: *Family → Invite member*. A **QR code** and a **link** appear.
   *Copy link* puts the link on the clipboard so it can be sent by Signal or
   any messenger. The code expires after five minutes.
3. Mona: *Join with an invitation*, paste the link, *Ask to join*. Both phones
   show the same six digits; Patrick confirms. Repeat for Mara. (A phone scans
   the QR code with its camera; the desktop windows have no scanner, so paste
   the link there. `target/debug/tackly 'tackly://join?c=…'` opens straight on the form.)
4. Add tasks, open one and tap *Start*, tick them off, and watch the other
   windows update live.

`just start 1` opens one window; `just reset` forgets all test data;
`just server` runs only the relay. The desktop build fakes the GPS with
`TACKLY_LOCATION` (each window gets its own).

| Recipe | |
| --- | --- |
| `just test` | unit tests and the device-level end-to-end suite |
| `just acceptance` | the Cucumber suite in real windows |
| `just lint` | clippy on everything |
| `just css` | compile the styles (Tailwind) after changing class names; CI checks they are up to date |
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

**Styles.** The screens use Tailwind classes only (`crates/app/src`), plus
`crates/app/tailwind.css` for the few things Tailwind can't say (the accent color,
three animations, the icon font class). `just css` compiles both into
`crates/app/src/style.css`, which is committed, so a plain `cargo build` and
Android need no Tailwind. Roboto and the Material Symbols icons the app uses are
in `crates/app/fonts/` and compiled into the app: nothing is loaded from the
network. The prototype the screens were built from is `prototype/index.html`.

### How it works

- **CQRS / event sourcing** with [`cqrs-es`](https://doc.rust-cqrs.org). The
  family is one aggregate (`crates/protocol/src/aggregate.rs`): commands such as
  `AssignTask`, `StartTask` or `CompleteTask` are checked against the replayed
  state and produce `FamilyEvent`s. Time worked is the sum of start/pause
  sessions since the task was (re)opened. A finish records where the phone was
  (not shown in the app). Who acts, the clock and new event IDs come from a
  `CommandContext`, so tests can fix them.
- **Offline first.** Every command commits to the phone's SQLite log first.
  A family can be created and used with no server; the phone registers and
  uploads later. Pairing needs the server.
- **Live updates.** The app keeps an SSE connection
  (`GET /v1/families/{id}/stream`) and reconnects from its cursor with backoff.
- **Finished twice.** Two members finishing the same task while apart is simply
  done; the first finish the relay received counts. Nobody is asked.
- **Encrypted.** Events are encrypted on the phone with a family key
  (AES-256-GCM). The server stores ciphertext and routing IDs only. The family
  key travels to a new member sealed under the invitation secret.

### Tests

All run against a real server with a SQLite file.

- `just test`: no windows. CI runs it on Linux.
  - `crates/protocol/tests/aggregate.rs`: given these events, when someone does
    this, then these events (or this error), with cqrs-es's test framework.
  - `crates/protocol/tests/projection.rs`: given these events, then the family
    every phone shows.
  - `crates/client/tests/e2e.rs`: a real relay and three real phones (live
    sync, server outage, offline start, lists, places, giving tasks away,
    photos).
- `just acceptance`: Cucumber scenarios in
  `crates/acceptance/tests/features/`. Three real app windows (Patrick, Mona,
  Mara); typing key by key, clicking, pasting the link, decoding the on-screen
  QR code. Playwright-style API: strict locators, auto-waiting actions,
  retrying `expect`. The engine runs inside the page (`tests/support/driver.js`)
  via the `ui-test` bridge, which exists only in test builds. Windows open and
  close alone; let it finish. CI runs it on macOS.
- `just android-test`: Playwright on an emulator (`android-e2e/suite.mjs`).
  The other phone is `tackly-probe` (in `crates/testkit`), a family head
  without a screen: it invites the emulator, lets it in and reports what
  synced. The test joins, taps Android's own location dialog, sets the
  emulator's GPS, ticks a task off (the probe gets where), and picks a photo
  (the probe gets it). Not in CI. Limits: `AndroidInput.type` drops capitals;
  `uiautomator dump` fails while Playwright's driver runs, use its device
  selectors.

Server details are in [server/README.md](server/README.md).

### Places and address search

Location autocomplete asks a [Photon](https://photon.komoot.io) server (OpenStreetMap
data); the typed text is sent there and nothing else. Offline, or with no match,
the typed text is kept as the location's name. `TACKLY_GEOCODER` points at
another server; tests use a fake one. Tasks, places and other IDs inside
encrypted events are UUIDv7. The four IDs the server sees (family, device,
event, invitation) are random UUIDv4, because a v7 ID would reveal when
something happened.

### Status

Not done, by decision: there is no restore process. A lost phone means
leaving and rejoining with a new invitation. Also missing: device revocation,
key rotation, scanning the QR code with the camera, opening invitation links
on Android, and keeping secrets in the Android Keystore (they sit in the
app-private storage for now). The location permission is declared in
`crates/app/Dioxus.toml`, so the store listing shows it; Android still asks
once, after creating or joining a family.
