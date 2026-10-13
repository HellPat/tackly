#!/usr/bin/env bash
# `just coverage`: every line of the given crates' sources runs in some test
# (their own tests; the phone's include the end-to-end suite against a relay).
# Lists each line no test runs and fails if there is one.
#
# Read from LCOV, not from llvm-cov's summary: a crate compiled for its own
# unit tests and again for its integration tests is counted once per
# compilation there, so a line that runs in only one of them looks missed.
#
#   scripts/coverage.sh tackly-domain tackly-wire tackly-phone tackly-agent
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
packages=()
for package in "$@"; do packages+=(-p "$package"); done
report="$(mktemp)"
trap 'rm -f "$report"' EXIT

cargo llvm-cov clean --workspace >/dev/null
cargo llvm-cov --locked "${packages[@]}" --lcov --output-path "$report"

missed="$(awk -F'[:,]' -v root="$root/" '
  /^SF:/ { file = substr($0, 4); sub(root, "", file) }
  /^DA:/ && $3 == 0 { print file ":" $2 }
' "$report")"
lines="$(awk -F'[:,]' '/^DA:/ { n++ } END { print n }' "$report")"
if [ -n "$missed" ]; then
  echo "Lines no test runs:"
  while IFS=: read -r file line; do
    printf '  %s:%s  %s\n' "$file" "$line" "$(sed -n "${line}p" "$file" | sed 's/^ *//')"
  done <<<"$missed"
  exit 1
fi
echo "All $lines lines run in a test."
