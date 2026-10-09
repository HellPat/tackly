# Tackly Android app

The Flutter app keeps one phone's tasks in an app-private SQLite event log.
Every saved change appends an event; screens replay events to derive lists and
tasks. Every task belongs to one list. Completion records the tap time and
available coordinates, even if the phone is offline. Missing location never
blocks completion.

You can use the app on one phone without a server. To share with your wife,
open **Family**, create a family with your sync server's HTTPS URL, save the
displayed recovery key in a password manager, and show the five-minute QR to
her phone. On her phone, open **Family** and scan the code. Compare the six-digit
confirmation code on both phones before allowing the request. Each installation
connects to at most one family. The server
has no UI; its deployment steps are in [server/README.md](../server/README.md).

The local event body is encrypted with an Android-secured key. For sync, event
bodies are encrypted again under the family key and stored in a durable outbox
before upload. The server has neither key. The family key and bearer token are
kept in Android secure storage. The database contains events and sync metadata,
not mutable task projections. Server-confirmed sequence defines replay order;
pending offline events follow it, with competing task edits shown for a choice.

The app tries to sync after local changes, on resume, every 20 seconds while
open, and through network-constrained Android WorkManager jobs. Android can
delay background jobs, so opening the app or tapping **Sync now** is the
fastest way to reconcile. The app shows a small message when the server cannot
be reached. Camera photos are resized and embedded in encrypted events.

The app disables Android backup. Uninstalling it removes local data and secure
keys. A saved recovery key lets a fresh installation reconnect to server-held
events. Existing debug APKs may use a different signing key, so a new APK may
not install over them; do not uninstall a phone with unsynced or otherwise
unrecoverable data.

## Build

Install Flutter, Android SDK, and JDK 21, then run:

```sh
cd app
flutter pub get
flutter run
```

The project currently contains an Android platform host only. Location
permission is requested at completion; there is no background location
permission. The UI uses Flutter Material controls and the Android camera app.

The emulator integration suites are `integration_test/current_slice_test.dart`
for local task flows, `integration_test/device_location_test.dart` for
Android GPS capture and the location-service fallback, and `integration_test/family_sync_test.dart`
for encrypted offline edits and conflict-choice UI with a deterministic relay.
`current_slice_test.dart` also checks list/task camera-result storage with a
controlled image picker result; it does not drive the external Android camera
app.
`integration_test/live_relay_test.dart` exercises client encryption, family
pairing, offline event upload, completion and recovery against the real Rust
server and PostgreSQL. It simulates two phones as separate event stores in one
emulator process; it does not test camera capture or two physical phones.
The release workflow runs these emulator suites on main pushes.
