#!/bin/sh
# Builds the relay for Workers. The tests build it once and set
# TACKLY_RELAY_PREBUILT, so that each `wrangler dev` they start skips this.
# Where Rust is missing (the Workers Builds image), it is installed first.
set -eu
[ -n "${TACKLY_RELAY_PREBUILT:-}" ] && exit 0
if ! command -v worker-build >/dev/null 2>&1; then
  if ! command -v cargo >/dev/null 2>&1; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy
    . "$HOME/.cargo/env"
  fi
  rustup target add wasm32-unknown-unknown
  cargo install --locked worker-build --version 0.8.7
fi
exec worker-build --release
