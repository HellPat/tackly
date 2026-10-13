#!/usr/bin/env bash
# Android helpers for the dev shell.
#
#   scripts/android.sh build   debug APK
#   scripts/android.sh run     build, install and start it on the running
#                              emulator or device (boots the first AVD if none)
#   scripts/android.sh emulate run in an emulator window; closing it (or
#                              Ctrl-C here) shuts the emulator down too
#   scripts/android.sh test    run + the Playwright-on-Android suite
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
package="dev.tackly.tackly"
activity="$package/dev.dioxus.main.MainActivity"
apk="$root/target/dx/tackly/debug/android/app/app/build/outputs/apk/debug/app-debug.apk"

build() {
  (cd "$root/apps/app" && dx build --android --package tackly-app --no-default-features --features mobile)
  # dx writes its own template's icon: put Tackly's in and build the APK again.
  "$root/scripts/android-icon.sh" "$root/target/dx/tackly/debug/android/app" >/dev/null
}

device_is_ready() {
  [[ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]
}

# Uses a connected device, else boots the first AVD without a window.
ensure_device() {
  device_is_ready && return
  local avd
  avd="$(emulator -list-avds | head -n 1)"
  [[ -n "$avd" ]] || { echo 'No device on adb and no AVD. Create one in Android Studio.' >&2; exit 1; }
  echo "Booting ${avd}…"
  nohup emulator -avd "$avd" -no-window -no-audio -no-boot-anim -no-snapshot \
    -gpu swiftshader_indirect >"$root/.dev/emulator.log" 2>&1 </dev/null &
  for _ in {1..90}; do device_is_ready && return; sleep 2; done
  echo "The emulator did not finish booting; see .dev/emulator.log." >&2
  exit 1
}

install_and_start() {
  adb install -r "$apk"
  adb shell am start -W -n "$activity"
}

# Opens the first AVD with its window, runs the app on it and stays until the
# emulator is closed or this is interrupted, which closes the emulator.
emulate() {
  local avd
  avd="$(emulator -list-avds | head -n 1)"
  [[ -n "$avd" ]] || { echo 'No AVD. Create one in Android Studio.' >&2; exit 1; }
  device_is_ready && { echo 'An emulator or device is already running; use just android.' >&2; exit 1; }
  emulator -avd "$avd" -no-snapshot >"$root/.dev/emulator.log" 2>&1 </dev/null &
  local pid=$!
  trap 'kill "$pid" 2>/dev/null || true' EXIT
  trap 'exit 130' INT TERM
  build
  for _ in {1..90}; do device_is_ready && break; kill -0 "$pid" 2>/dev/null || exit 1; sleep 2; done
  install_and_start
  echo "Running on ${avd}. Close the emulator or press Ctrl-C to stop."
  wait "$pid" || true
}

mkdir -p "$root/.dev"
case "${1:-}" in
  build) build ;;
  emulate) emulate ;;
  run) build; ensure_device; install_and_start ;;
  test)
    build
    ensure_device
    cargo build --locked -p tackly-testkit --bins
    adb install -r "$apk"
    cd "$root/tests/android-e2e"
    npm ci --no-audit --no-fund
    npx playwright install android
    node suite.mjs
    ;;
  *) echo "usage: $0 build|run|emulate|test" >&2; exit 2 ;;
esac
