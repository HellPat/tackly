# Current-scope end-to-end coverage

This table covers only the implemented Android Flutter app and the headless
encrypted sync server. “Emulator” means the Flutter integration test runs in
an installed Android app. “Live relay” means the Flutter client talks to the
real Rust API and disposable PostgreSQL; its two phones are simulated as
separate local event stores in one emulator process.

| Implemented behavior | End-to-end check |
|---|---|
| Lists, required task parent, All Tasks, task creation/edit, emoji choice, completion and Revert | app/integration_test/current_slice_test.dart on emulator |
| List and task photo choice, image storage, and replay | app/integration_test/current_slice_test.dart on emulator with a controlled image-picker result |
| Local encrypted event replay after database reopen; rapid completion/reversal | app/integration_test/current_slice_test.dart on emulator |
| Completion time and coordinates when supplied by location service | app/integration_test/current_slice_test.dart with deterministic location and app/integration_test/device_location_test.dart with Android's GPS test provider |
| Completion time when Android location service is disabled | app/integration_test/device_location_test.dart on emulator |
| Offline concurrent edits, task-only conflict, visible choice, convergence | app/integration_test/family_sync_test.dart on emulator with deterministic HTTP relay |
| QR invitation, owner approval, one family per phone, offline upload, ciphertext-only relay, completion sync, member recovery | app/integration_test/live_relay_test.dart on emulator with real Rust server and PostgreSQL |
| Family screen create, recovery-key display, invite and cancellation, manual sync, and restore controls | app/integration_test/family_ui_test.dart on emulator with real Rust server and PostgreSQL |
| Family creation/recovery, tenant isolation, API authorization, event validation, batching, paging, retry idempotency | server/tests/api_e2e.py against real server and PostgreSQL |
| One-use joining, invitation expiry, owner/member permissions, cancellation at each stage, concurrent event order | server/tests/api_e2e.py against real server and PostgreSQL |
| Accepted events reject update/delete/truncate | server/tests/retention.sql against PostgreSQL |

The server transport smoke path in server/tests/transport_smoke.py is a
second, compact check of the primary API flow. The Android CI workflow runs
the emulator cases above; the server workflow runs the API and retention
checks.

## Evidence boundaries

- The photo flow is checked with a controlled image-picker result, including
  event storage and replay. Automated capture through the external Android
  camera app is not yet covered.
- The live relay test simulates two phones inside one emulator process. It
  validates client/server integration, not two physical phones, real QR
  scanning, background delivery timing, or a server outage lasting across app
  process restarts.
- Both location-disabled and captured-location platform paths run on the
  emulator in CI. The latter uses Android's temporary GPS test provider and
  shell-granted location permission, then removes the provider.
- The older Today, places, timers, recommendations, revocation/key rotation,
  MCP, and iOS scenarios are outside the implemented feature set. Their
  historical Gherkin files are not passing product tests.
