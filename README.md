# Tackly

An Android-only, single-user Flutter task app. Lists contain tasks. Each task
can be checked off; its completion event records the time and available phone
coordinates. All saved changes are append-only events in an app-private SQLite
database, and the visible state is rebuilt by replaying those events. There is
no account, sharing, server, or sync in this slice.

The app and data model are documented in [app/README.md](app/README.md).

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

## Build and test locally

Use Flutter 3.47.7, Android SDK, and JDK 21:

```sh
cd app
flutter pub get
flutter analyze
flutter test integration_test/current_slice_test.dart -d <android-device-id>
flutter build apk --debug
```

The emulator suite covers list creation, task assignment, emoji choice, edit by
long press, completion, Revert, event replay after reopening SQLite, and rapid
repeat actions. `integration_test/device_location_test.dart` separately checks
the real Android location plugin with `EXPECT_LOCATION=service_disabled` or
`EXPECT_LOCATION=captured` after configuring an emulator's location and runtime
permission. The captured-GPS variant is not a CI gate because the local API 36
emulator has timed out despite simulated fixes; completion correctly records
`unavailable` in that case.
