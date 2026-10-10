# One-command Android development session. Override TACKLY_AVD to choose an AVD.
start:
    #!/usr/bin/env bash
    set -euo pipefail

    repo='{{ justfile_directory() }}'
    sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
    adb="${ADB_BIN:-$sdk/platform-tools/adb}"
    emulator="${EMULATOR_BIN:-$sdk/emulator/emulator}"
    flutter="${FLUTTER_BIN:-flutter}"
    server_pid=''

    cleanup() {
      if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
      fi
    }
    trap cleanup EXIT

    for tool in "$adb" "$emulator" "$flutter" cargo curl; do
      if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing $tool. Install Flutter, Rust, and the Android SDK, or set FLUTTER_BIN/ADB_BIN/EMULATOR_BIN." >&2
        exit 1
      fi
    done

    health() {
      [[ "$(curl --silent --max-time 2 --output /dev/null --write-out '%{http_code}' http://127.0.0.1:3000/health || true)" == 204 ]]
    }

    if health; then
      echo 'Using the local Tackly server on 127.0.0.1:3000.'
    else
      if lsof -nP -iTCP:3000 -sTCP:LISTEN >/dev/null 2>&1; then
        echo 'Port 3000 is in use, but the Tackly health endpoint is unavailable.' >&2
        exit 1
      fi
      db="sqlite://$repo/server/tackly-sync.db"
      (cd "$repo/server" && DATABASE_URL="$db" cargo run --locked -- migrate)
      (
        cd "$repo/server"
        exec env DATABASE_URL="$db" TACKLY_BIND=127.0.0.1:3000 ./target/debug/tackly-sync
      ) >"$repo/server/target/tackly-dev-server.log" 2>&1 &
      server_pid=$!
      for _ in {1..30}; do
        if health; then break; fi
        if ! kill -0 "$server_pid" 2>/dev/null; then break; fi
        sleep 1
      done
      if ! health; then
        cat "$repo/server/target/tackly-dev-server.log" >&2
        echo 'Could not start the local Tackly server.' >&2
        exit 1
      fi
      echo 'Started the local Tackly server on 127.0.0.1:3000.'
    fi

    running_emulator() {
      "$adb" devices | awk '$1 ~ /^emulator-[0-9]+$/ && $2 == "device" { print $1; exit }'
    }

    serial="$(running_emulator)"
    if [[ -z "$serial" ]]; then
      avd="${TACKLY_AVD:-$("$emulator" -list-avds | head -n 1)}"
      if [[ -z "$avd" ]]; then
        echo 'No Android emulator found. Create an AVD in Android Studio first.' >&2
        exit 1
      fi
      echo "Starting Android emulator $avd…"
      nohup "$emulator" -avd "$avd" >"$repo/server/target/tackly-emulator.log" 2>&1 </dev/null &
    fi

    for _ in {1..120}; do
      serial="$(running_emulator)"
      if [[ -n "$serial" ]] && [[ "$("$adb" -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; then
        break
      fi
      sleep 2
    done
    if [[ -z "$serial" ]] || [[ "$("$adb" -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" != 1 ]]; then
      echo "Emulator did not finish booting. See server/target/tackly-emulator.log." >&2
      exit 1
    fi

    echo "Opening Tackly on $serial, connected to the local server."
    cd "$repo/app"
    "$flutter" run -d "$serial" --dart-define=TACKLY_DEV_SERVER_URL=http://10.0.2.2:3000
