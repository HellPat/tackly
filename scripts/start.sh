#!/usr/bin/env bash
# `just start`: the relay plus N family members, each in its own window with
# its own database. Data stays in .dev/ between runs.
#
#   scripts/start.sh [members]
set -euo pipefail

members="${1:-3}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .dev

# Patrick is the head of the family: his window creates it.
names=(Patrick Mona Mara Dana Eli)
places=("52.5200,13.4050" "52.5163,13.3777" "52.5075,13.3904" "52.4900,13.3600" "52.5300,13.4200")

cargo build --locked -p tackly-sync -p tackly-app

pids=()
stop_everything() {
  kill "${pids[@]}" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap stop_everything EXIT INT TERM

relay_is_up() {
  [[ "$(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$1/health" || true)" == 204 ]]
}

# The first port from 3000 that already runs the relay, or else a free one.
choose_port() {
  local candidate
  for candidate in {3000..3010}; do
    if relay_is_up "$candidate"; then echo "$candidate"; return; fi
    if ! lsof -nP -iTCP:"$candidate" -sTCP:LISTEN >/dev/null 2>&1; then echo "$candidate"; return; fi
  done
  return 1
}

port="${TACKLY_SERVER_PORT:-$(choose_port)}" || { echo 'No free port from 3000 to 3010.' >&2; exit 1; }

if relay_is_up "$port"; then
  echo "Using the Tackly server already running on 127.0.0.1:$port."
else
  DATABASE_URL="sqlite://$root/.dev/relay.db" TACKLY_BIND="127.0.0.1:$port" \
    target/debug/tackly-sync >.dev/server.log 2>&1 &
  pids+=($!)
  for _ in {1..30}; do relay_is_up "$port" && break; sleep 0.5; done
  relay_is_up "$port" || { cat .dev/server.log >&2; echo 'The server did not start.' >&2; exit 1; }
  echo "Started the Tackly server on 127.0.0.1:$port (log: .dev/server.log)."
fi

app_pids=()
for ((i = 0; i < members && i < ${#names[@]}; i++)); do
  name="${names[$i]}"
  TACKLY_PROFILE="$name" TACKLY_DATA_DIR="$root/.dev/$name" \
    TACKLY_SERVER_URL="http://127.0.0.1:$port" TACKLY_LOCATION="${places[$i]}" \
    target/debug/tackly-app >".dev/$name.log" 2>&1 &
  pids+=($!)
  app_pids+=($!)
done

echo "Windows: ${names[*]:0:members}. Patrick: Create a family, then Family > Invite someone."
echo "Ctrl-C stops everything. 'just reset' forgets all test data."
wait "${app_pids[@]}"
