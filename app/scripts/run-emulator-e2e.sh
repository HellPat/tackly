#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

flutter test integration_test/current_slice_test.dart
flutter test integration_test/family_sync_test.dart
flutter test integration_test/live_relay_test.dart --dart-define=TACKLY_E2E_SERVER=http://10.0.2.2:3000
flutter test integration_test/family_ui_test.dart --dart-define=TACKLY_E2E_SERVER=http://10.0.2.2:3000

adb shell settings put secure location_mode 0
flutter test integration_test/device_location_test.dart --dart-define=EXPECT_LOCATION=service_disabled

adb shell settings put secure location_mode 3
adb shell appops set 2000 android:mock_location allow
adb shell cmd location providers add-test-provider gps
adb shell cmd location providers set-test-provider-enabled gps true

cleanup_location_provider() {
  adb shell cmd location providers remove-test-provider gps || true
  adb shell appops set 2000 android:mock_location default || true
}
trap cleanup_location_provider EXIT

flutter test integration_test/device_location_test.dart --dart-define=EXPECT_LOCATION=captured &
location_test_pid=$!
while kill -0 "$location_test_pid" 2>/dev/null; do
  adb shell pm grant dev.tackly.tackly android.permission.ACCESS_FINE_LOCATION >/dev/null 2>&1 || true
  adb shell cmd location providers set-test-provider-location gps --location 52.52,13.405 --accuracy 5 >/dev/null 2>&1 || true
  sleep 2
done
wait "$location_test_pid"
