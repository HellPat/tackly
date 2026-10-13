# Tackly development. `just start` is all you need.
#
# Every recipe runs inside the Nix dev shell when Nix is installed
# (scripts/dev-shell.sh); the real work is in scripts/.
set shell := ["bash", "-euo", "pipefail", "-c"]

shell := justfile_directory() / "scripts/dev-shell.sh"
scripts := justfile_directory() / "scripts"

# Run the sync server and three family members (Patrick, Mona, Mara), each in
# its own window with its own database. Data lives in .dev/ between runs.
start members="3":
    {{shell}} {{scripts}}/start.sh {{members}}

# Forget all local test data (families, tasks, relay database).
reset:
    rm -rf .dev

# Run only the relay (the Cloudflare Worker under wrangler dev) on 127.0.0.1:3000.
server:
    {{shell}} {{scripts}}/relay.sh

# Fast tests without windows: unit tests and the device-level end-to-end suite
# (against the relay under wrangler dev, one per test).
test: relay-deps
    {{shell}} cargo test --locked -p tackly-domain -p tackly-wire -p tackly-phone

# The end-to-end suite against the deployed relay (tests that must stop the
# relay or play a hostile one skip themselves).
test-deployed url="https://relay.tackly.hellpat.com":
    {{shell}} env TACKLY_RELAY_URL={{url}} cargo test --locked -p tackly-phone --test e2e

# wrangler for the relay's tests and `just server`.
relay-deps:
    cd apps/relay && npm ci --no-audit --no-fund --silent

# Cucumber acceptance tests: three real app windows (Patrick, Mona, Mara) are
# opened per scenario and clicked through, against a real server. Windows pop
# up and move on their own while this runs; let it finish.
acceptance: relay-deps
    {{shell}} cargo test --locked -p tackly-acceptance --test acceptance

# Every line of domain, wire, phone and agent runs in their tests.
coverage: relay-deps
    {{shell}} bash -c 'for crate in tackly-domain tackly-wire tackly-phone tackly-agent; do {{scripts}}/coverage.sh "$crate"; done'

# Mutation testing of the fast crates: every mutant must make a test fail.
mutants: relay-deps
    {{shell}} env TACKLY_RELAY_DIR={{justfile_directory()}}/apps/relay cargo mutants -p tackly-domain -p tackly-wire -p tackly-agent --timeout 300 --cargo-arg=--locked

# Compile the app's styles (Tailwind) into apps/app/src/style.css. Run after
# changing class names; CI checks the result is up to date.
css:
    {{shell}} tailwindcss --input apps/app/tailwind.css --output apps/app/src/style.css --minify

# Clippy on everything, the relay (WebAssembly) included. Unwrap and expect
# are denied (see Cargo.toml).
lint:
    {{shell}} cargo clippy --workspace --all-targets --locked -- -D warnings
    cd apps/relay && {{shell}} cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings

# Debug APK for Android (needs the Android SDK and NDK; the shell has the rest).
android-build:
    {{shell}} {{scripts}}/android.sh build

# Build, install and start the app on the running emulator or device.
android:
    {{shell}} {{scripts}}/android.sh run

# Open the emulator with its window and run the app on it; exiting here closes the emulator.
emulate:
    {{shell}} {{scripts}}/android.sh emulate

# The Playwright-on-Android suite: real taps and keyboard in an emulator.
android-test:
    {{shell}} {{scripts}}/android.sh test
