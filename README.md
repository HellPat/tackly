# Tackly

An Android Flutter task app with a headless encrypted sync server. Lists contain
tasks. Each task can be checked off; its completion event records the time and
available phone coordinates. Saved changes are append-only events in an
app-private SQLite database, and visible state is rebuilt by replaying them.
The optional SQLite server relays encrypted events between two phones in
one family. A family and its tasks can be created without a server; enter a
server URL later from the invitation screen when ready to share. New task names
search existing tasks locally, and selecting a completed match reopens it.

The app and data model are documented in [app/README.md](app/README.md), and
server setup and security limits in [server/README.md](server/README.md).

## Run locally

With `just`, Flutter, Rust, and an Android SDK/AVD installed, run `just start`
from the repository root. It reuses a local Tackly server on port 3000 or
starts one with `server/tackly-sync.db`, opens an Android emulator, and runs the
debug app against `http://10.0.2.2:3000`. New family dialogs use this address;
an existing owner family without a server connects to it on launch. A family
already connected to another server keeps its existing address. The app still
works when the server is unavailable. Set `TACKLY_AVD` to select a different
emulator. Stop `just start` with Ctrl-C; a server it started stops with it.

## Install a build from GitHub

1. Open [GitHub Releases](https://github.com/HellPat/tackly/releases) and
   download the APK from the latest release.
2. Open the APK on an Android phone. Android may ask you to allow installs
   from the app you used to open it.

Alternatively, with Android Debug Bridge connected:

```sh
adb install tackly-android-<release-number>.apk
```

The current releases use a stable signing key. The earlier debug preview APK
cannot be upgraded in place to a release APK; uninstalling the debug version
deletes its local data because Android backup is disabled. Save the recovery
key before replacing an installation with important data.

## Continuous GitHub releases

After the four signing secrets below are configured, each push to `main` runs
the emulator suite, builds a signed release APK with a new Android version code,
and publishes it at [GitHub Releases](https://github.com/HellPat/tackly/releases).
The release job fails clearly if signing is not configured. The existing debug
APK cannot be upgraded to the first release APK because its signing key differs;
uninstalling it deletes the local SQLite database. Keep the release key backed
up: future APKs need the same signing key to update in place.

The required repository Actions secrets are:

- `TACKLY_KEYSTORE_BASE64`: base64 encoding of a private Android keystore
- `TACKLY_STORE_PASSWORD`: keystore password
- `TACKLY_KEY_ALIAS`: key alias in that keystore
- `TACKLY_KEY_PASSWORD`: key password

The keystore and passwords must not be committed. `app/android/key.properties`
is ignored locally. GitHub Releases do not automatically install updates on a
phone. Google Play internal testing can do that later; moving a GitHub install
to Play without losing app data requires the same Android app signing identity.

## Build and test locally

Use Flutter 3.47.7, Android SDK, and JDK 21:

```sh
cd app
flutter pub get
flutter analyze
flutter test integration_test/current_slice_test.dart -d <android-device-id>
flutter build apk --debug
```

The emulator suite covers local task flows, controlled camera results, offline
conflicts, and a Flutter-to-Rust-to-SQLite live relay. Android location
integration runs both disabled-service and captured-coordinate cases; the
captured case uses a temporary Android GPS test provider. The server workflow
also checks every current HTTP endpoint and append-only retention. See
[current end-to-end coverage](acceptance/CURRENT-SCOPE-E2E.md) for the exact
drivers and remaining device-level gaps.
