# Tackly Android app

This is the current single-user, Android-only first slice. It has no account,
family, sharing, server connection, recommendations, timer, or background
location tracking.

## What it does

- Create lists, each with an emoji or a camera photo.
- Add tasks to a list, with an emoji or camera photo. Every task belongs to one
  list. The root shows an All Tasks view and the open count for each list.
- Press and hold an open task to edit its name or image.
- Check a task to complete it. A completion event records the time of the tap
  and the phone's coordinates and accuracy when available. If location is
  denied, disabled, or times out, completion still succeeds and records that
  status. The confirmation offers Revert, which appends another event.

## Local data model

The app-private SQLite database is `tackly_events.db`. Its single `events`
table has a monotonic local sequence, UUID event ID, schema version, aggregate
ID, event type, UTC event time, and JSON payload. Each saved change appends an
event: `list.created`, `task.created`, `task.updated`, `task.completed`, or
`task.completion_reverted`. The UI reads all events in sequence and derives its
list/task state in memory. It stores no mutable task or list projection table.
Navigation, typing before Save, and canceled camera actions are not domain
events.

Camera images are resized and embedded in the relevant event as Base64 so the
event contains the task/list image without relying on a temporary camera path.
Only local SQLite storage is implemented. A future PostgreSQL server can store
and relay these events, but synchronization and conflict rules are not part of
this app. The Android app disables system backup of its local database.

## Build

Install Flutter, Android SDK, and JDK 21, then run:

```sh
cd app
flutter pub get
flutter run
```

For the emulator end-to-end suite, run
`flutter test integration_test/current_slice_test.dart -d <android-device-id>`.
The GitHub Actions workflow runs this suite and publishes a debug APK.

The project contains only an Android platform host. Device location permission
is requested when the first task is completed; no background permission is
requested. The UI uses Flutter's Material controls and the Android camera app.
