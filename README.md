# Tackly

ADHD optimized task management. Shared family tasks that work offline and sync live.

## What it does

- *Today*: your three priorities, in your order. Drag to reorder. Free slot?
  *Pick something*. All done: a kitten celebrates. What you did today shows as a win.
- *Upcoming*: at your planning time (20:00, set in your settings) it opens by
  itself. Fill three slots from your problems and what is left over: tap ↑ or
  drag. Swipe right: not this time. *Save plan* or *Plan later*.
- *Problems*: "Out of food", "Laundry piles up". *Notice* it for you or *Raise*
  it for someone; its solutions become steps. *Resolve* by hand. Old problems
  come back as suggestions.
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
- Encrypted on the phones, end to end (MLS, the messaging standard). The
  server cannot read your tasks; someone who leaves cannot read what comes after.

Android first, iOS maybe later. No restore: lost phone means join again.

---

# Development

```
apps/app           the Dioxus app (desktop windows and Android), styled with Tailwind;
                   one screens folder per area (today, problems, tasks, places, family)
apps/relay         the server: stores encrypted messages, pushes them over SSE
apps/agent         tackly-agent, the command line tool an AI agent reads along with
domain/            the rules, no network or disk: the Family aggregate (cqrs-es), split
                   into members, tasks, places and problems
phone/             what a phone does without a screen: use cases (device), keeping in
                   sync (live), and infrastructure (SQLite log, MLS group, relay client,
                   key files, address search)
wire/              the messages between phone and relay (HTTP and SSE payloads)
tests/testkit      a stoppable real relay, shared by the test suites
tests/acceptance   Cucumber scenarios clicked through real app windows
tests/android-e2e  Playwright on an Android emulator (see below)
scripts            what the just recipes run
```

The arrows point one way: `apps/app` and `apps/agent` use `phone`; `phone` uses
`domain` and `wire`; `apps/relay` uses only `wire`; `domain` uses nothing.

### Try it

Install [Nix](https://nixos.org/download) (flakes enabled), then:

```sh
just start
```

With [direnv](https://direnv.net), `direnv allow` once: the dev shell then
loads on entering the folder, cached by nix-direnv (the first time takes a
minute or two; after that, under a second).

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
`cargo run -p tackly-relay` and `cargo run -p tackly-app` (binary `tackly`) yourself.

**No unwrap.** Errors are handled, not unwrapped: `unwrap` and `expect` are
denied by clippy (tests may use them). `.cargo/config.toml` runs clippy on every
workspace build, so a stray `unwrap()` fails plain `cargo build`, not only
`cargo clippy`.

**Styles.** The screens use Tailwind classes only (`apps/app/src`), plus
`apps/app/tailwind.css` for the few things Tailwind can't say (the accent color,
three animations, the icon font class). `just css` compiles both into
`apps/app/src/style.css`, which is committed, so a plain `cargo build` and
Android need no Tailwind. Roboto and the Material Symbols icons the app uses are
in `apps/app/fonts/` and compiled into the app: nothing is loaded from the
network. The prototype the screens were built from is `prototype/index.html`.

### How it works

- **CQRS / event sourcing** with [`cqrs-es`](https://doc.rust-cqrs.org). The
  family is one aggregate (`domain/src/family`): commands such as
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
- **Only https on the internet.** The device token travels as a bearer token,
  so a relay address must be `https://`; plain `http://` is accepted only on
  the person's own network (localhost, private addresses, names without a dot
  or ending in `.local`), such as the Android emulator's `10.0.2.2`.
- **Values with rules.** Anything that has rules is parsed once, at the edge,
  into a value object that cannot be invalid, and nothing downstream checks
  again: `Name`, `Photo`, `GeoPoint`, `Focus` and `IconName` in
  `domain/src/values`; `ServerUrl`, `RecoveryCode`, `InviteLink` and
  `DeviceToken` in `phone`; `Bytes` and `Digest` for binary values on the
  wire; `TimeOfDay` and `SchemeId` for the app's settings. They have private
  fields, `FromStr`/`Display`, and tests; events from other phones are checked
  when they are deserialized, and the ones that hold secrets stay out of `Debug`.
- **Encrypted.** The family is an MLS group (RFC 9420, via OpenMLS). Every
  event is an MLS message: encrypted to the group's current key, signed by
  the phone that wrote it, numbered per phone. Adding a member is a commit
  that moves the group to a new key; the new phone gets a welcome, then the
  whole history as a snapshot from the head. The server stores messages and
  routing IDs, and reads only each message's epoch and kind. Both phones
  derive the six join digits from the invitation secret and the joiner's MLS
  key package, so a server that swaps the key is caught (`phone/tests/e2e.rs`).
- **The head decides who is in.** Only the head's phone may commit (the
  server checks its token; every phone checks the commit's sender, so a
  stolen member phone cannot throw anyone out). Removing someone is a commit
  to a key that phone never gets, plus `member.removed` in the log; the server
  drops its token. The removed phone keeps what it had and sees nothing new.
- **AI agents read along, for a while.** The agent runs
  `tackly-agent login --server <relay> --family <family ID>` (the Family screen
  shows both and can copy the command). It prints a code of twelve digits and
  waits. The head's phone shows the request with the same code; the head
  compares them and allows access for 30 minutes, 2 hours or a day, or denies
  it. The code is a hash of the agent's key, so a relay that swapped the key
  would show another one. This is the OAuth device flow (RFC 8628) with MLS
  doing the encryption. The agent is a member with the whole history
  (`tackly-agent state|events|follow|logout`). It cannot write: the relay
  refuses its token for writes, and the phones ignore anything it sends. Its
  relay token ends at the time given; the head's phone takes it out of the
  group when that time has passed, or at once with Revoke. At most five
  requests wait for a family at once, for ten minutes.
- **Recovery code.** The recovery code is an [age](https://age-encryption.org)
  identity, made, written and checked by the `age` crate (a 256-bit X25519 key
  from the system's secure random generator, in Bech32 with a checksum that
  catches typos); it is shown once, to be kept in a password manager, and the
  phone forgets it when the head says it saved it. After every change the
  head's phone seals a backup to the code's public key (age: the head's MLS
  signing key, the group's public state, its relay token and the whole
  history) and leaves it with the server under an ID made from that public
  key. Only the code opens it, and the file is a standard age file.
  A new phone with the code takes over the head's identity, comes back into the group from outside (an MLS external commit, which also
  removes the lost phone's leaf; the others accept it because it is signed
  with the head's key), then moves to a fresh signing key and a fresh relay
  token, so the lost phone is out. The others send it what happened since
  the backup.

### Tests

All run against a real server with a SQLite file.

- `just test`: no windows. CI runs it on Linux.
  - `domain/tests/aggregate.rs`: given these events, when someone does
    this, then these events (or this error), with cqrs-es's test framework.
  - `domain/tests/projection.rs`: given these events, then the family
    every phone shows.
  - `phone/tests/e2e.rs`: a real relay and three real phones (live
    sync, server outage, offline start, lists, places, giving tasks away,
    photos, a raised problem and a day's plan).
- `just acceptance`: Cucumber scenarios in
  `tests/acceptance/tests/features/`. Three real app windows (Patrick, Mona,
  Mara); typing key by key, clicking, pasting the link, decoding the on-screen
  QR code, dragging by a handle, swiping. Playwright-style API: strict
  locators, auto-waiting actions, retrying `expect`. Locators are roles and
  labels people see (`get_by_role`, `get_by_label`), else a `data-testid`
  (`get_by_test_id`); no CSS. A test build reads `clock-offset-minutes` in the
  data folder, so "it is 20:00" and "the next morning" can be played. The engine runs inside the page (`tests/support/driver.js`)
  via the `ui-test` bridge, which exists only in test builds. Windows open and
  close alone; let it finish. CI runs it on macOS.
- `just android-test`: Playwright on an emulator (`tests/android-e2e/suite.mjs`).
  The other phone is `tackly-probe` (in `tests/testkit`), a family head
  without a screen: it invites the emulator, lets it in and reports what
  synced. The test joins, taps Android's own location dialog, sets the
  emulator's GPS, ticks a task off (the probe gets where), and picks a photo
  (the probe gets it). Not in CI. Limits: `AndroidInput.type` drops capitals;
  `uiautomator dump` fails while Playwright's driver runs, use its device
  selectors.

Server details are in [apps/relay/README.md](apps/relay/README.md).

### Places and address search

Location autocomplete asks a [Photon](https://photon.komoot.io) server (OpenStreetMap
data); the typed text is sent there and nothing else. Offline, or with no match,
the typed text is kept as the location's name. `TACKLY_GEOCODER` points at
another server; tests use a fake one. Tasks, places and other IDs inside
encrypted events are UUIDv7. The four IDs the server sees (family, device,
event, invitation) are random UUIDv4, because a v7 ID would reveal when
something happened.

### Status

Missing: scanning the QR code with the camera, opening invitation links on
Android, keeping secrets in the Android Keystore (they sit in the app-private
storage for now), and a second head (one phone holds the family; its recovery
code is the way back). The recovery backup is uploaded whole after every
change, which is fine for a family's history but not tuned. A member whose
phone is lost simply rejoins with a new invitation; only the head has a
recovery code. The location permission is declared in
`apps/app/Dioxus.toml`, so the store listing shows it; Android still asks
once, after creating or joining a family.
