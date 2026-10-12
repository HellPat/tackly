#!/usr/bin/env bash
# `just server`: only the relay, the Cloudflare Worker under `wrangler dev`
# (Worker, Durable Object and a local D1), on 127.0.0.1:3000 (or
# TACKLY_SERVER_PORT), data in .dev/relay/.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root/apps/relay"
[ -d node_modules ] || npm ci --no-audit --no-fund
mkdir -p "$root/.dev"
exec node_modules/.bin/wrangler dev --ip 127.0.0.1 --port "${TACKLY_SERVER_PORT:-3000}" \
  --persist-to "$root/.dev/relay" --show-interactive-dev-session=false
