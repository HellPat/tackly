#!/usr/bin/env bash
# Runs a command inside the project's Nix dev shell (Rust with the Android
# targets, JDK, Node, dx, just). Already inside it, or without Nix, the
# command runs as is.
#
#   scripts/dev-shell.sh cargo test
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -z "${TACKLY_DEV_SHELL:-}" ]] && command -v nix >/dev/null 2>&1; then
  exec nix develop "$root" --command "$@"
fi
exec "$@"
