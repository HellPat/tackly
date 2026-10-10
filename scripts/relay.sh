#!/usr/bin/env bash
# `just server`: only the relay, on 127.0.0.1:3000, data in .dev/relay.db.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .dev
DATABASE_URL="sqlite://$root/.dev/relay.db" exec cargo run --locked -p tackly-sync
