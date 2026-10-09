# Tackly

An Android Flutter task app with a headless encrypted sync server. Lists contain
tasks. Each task can be checked off; its completion event records the time and
available phone coordinates. Saved changes are append-only events in an
app-private SQLite database, and visible state is rebuilt by replaying them.
The optional PostgreSQL server relays encrypted events between two phones in
one family. The app remains usable offline.

The app and data model are documented in [app/README.md](app/README.md), and
server setup and security limits in [server/README.md](server/README.md).

## Install a build from GitHub

1. Open the latest successful **Android** run under this repository's **Actions**
   tab.
2. Download the `tackly-android` artifact and unzip it.
3. Move `app-debug.apk` to an Android phone and open it. Android may ask you to
   allow installs from the app you used to open the APK.

Alternatively, with Android Debug Bridge connected:

```sh
adb install app-debug.apk
```

The workflow currently publishes a **debug-signed** APK. Different workflow
runs can use different debug signing keys. An update from another run may fail
to install over the first APK; uninstalling the old app also deletes its local
database because backup is disabled. Do not rely on these artifacts to preserve
important data across updates. A stable release signing key needs a separate
approved setup before distributing updates.

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
conflicts, and a Flutter-to-Rust-to-PostgreSQL live relay. Android location
integration runs both disabled-service and captured-coordinate cases; the
captured case uses a temporary Android GPS test provider. The server workflow
also checks every current HTTP endpoint and append-only retention. See
[current end-to-end coverage](acceptance/CURRENT-SCOPE-E2E.md) for the exact
drivers and remaining device-level gaps.
