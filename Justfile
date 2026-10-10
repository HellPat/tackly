# Tackly development. `just start` is all you need.
set shell := ["bash", "-euo", "pipefail", "-c"]

# Run the sync server and three family members (Anna, Ben, Caro), each in
# its own window with its own database. Data lives in .dev/ between runs.
start members="3":
    #!/usr/bin/env bash
    set -euo pipefail
    repo='{{ justfile_directory() }}'
    if [[ -z "${TACKLY_DEV_SHELL:-}" ]] && command -v nix >/dev/null 2>&1; then
      exec nix develop "$repo" --command just start {{ members }}
    fi
    cd "$repo"
    mkdir -p .dev
    cargo build --locked -p tackly-sync -p tackly-app

    pids=()
    cleanup() { kill "${pids[@]}" 2>/dev/null || true; wait 2>/dev/null || true; }
    trap cleanup EXIT INT TERM

    health() { [[ "$(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$1/health" || true)" == 204 ]]; }
    port="${TACKLY_SERVER_PORT:-}"
    if [[ -z "$port" ]]; then
      for candidate in {3000..3010}; do
        if health "$candidate"; then port="$candidate"; break; fi
        if ! lsof -nP -iTCP:"$candidate" -sTCP:LISTEN >/dev/null 2>&1; then port="$candidate"; break; fi
      done
    fi
    [[ -n "$port" ]] || { echo 'No free port from 3000 to 3010.' >&2; exit 1; }
    if health "$port"; then
      echo "Using the Tackly server already running on 127.0.0.1:$port."
    else
      DATABASE_URL="sqlite://$repo/.dev/relay.db" TACKLY_BIND="127.0.0.1:$port" \
        target/debug/tackly-sync >.dev/server.log 2>&1 &
      pids+=($!)
      for _ in {1..30}; do health "$port" && break; sleep 0.5; done
      health "$port" || { cat .dev/server.log >&2; echo 'Server did not start.' >&2; exit 1; }
      echo "Started the Tackly server on 127.0.0.1:$port (log: .dev/server.log)."
    fi

    names=(Anna Ben Caro Dana Eli)
    places=("52.5200,13.4050" "52.5163,13.3777" "52.5075,13.3904" "52.4900,13.3600" "52.5300,13.4200")
    for ((i = 0; i < {{ members }} && i < 5; i++)); do
      name="${names[$i]}"
      TACKLY_PROFILE="$name" TACKLY_DATA_DIR="$repo/.dev/$name" \
        TACKLY_SERVER_URL="http://127.0.0.1:$port" TACKLY_LOCATION="${places[$i]}" \
        target/debug/tackly-app >".dev/$name.log" 2>&1 &
      pids+=($!)
    done
    echo "Windows: ${names[*]:0:{{ members }}}. Anna: Create a family, then Family > Invite someone."
    echo "Ctrl-C stops everything. 'just reset' forgets all test data."
    wait "${pids[@]:1}"

# Forget all local test data (families, tasks, relay database).
reset:
    rm -rf .dev

# Unit and end-to-end tests: real server, three phones, live SSE, outages.
test:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${TACKLY_DEV_SHELL:-}" ]] && command -v nix >/dev/null 2>&1; then
      exec nix develop '{{ justfile_directory() }}' --command just test
    fi
    cd '{{ justfile_directory() }}'
    cargo test --locked -p tackly-protocol -p tackly-client -p tackly-sync

# Run only the sync server on 127.0.0.1:3000.
server:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${TACKLY_DEV_SHELL:-}" ]] && command -v nix >/dev/null 2>&1; then
      exec nix develop '{{ justfile_directory() }}' --command just server
    fi
    cd '{{ justfile_directory() }}'
    mkdir -p .dev
    DATABASE_URL="sqlite://$PWD/.dev/relay.db" cargo run --locked -p tackly-sync

# Android emulator build (needs the Android SDK/NDK and an AVD). Not covered by CI.
android:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${TACKLY_DEV_SHELL:-}" ]] && command -v nix >/dev/null 2>&1; then
      exec nix develop '{{ justfile_directory() }}' --command just android
    fi
    cd '{{ justfile_directory() }}/crates/app'
    dx serve --platform android --no-default-features --features mobile
