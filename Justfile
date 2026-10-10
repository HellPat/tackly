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

# Run only the sync server on 127.0.0.1:3000.
server:
    {{shell}} {{scripts}}/relay.sh

# Fast tests without windows: unit tests and the device-level end-to-end suite.
test:
    {{shell}} cargo test --locked -p tackly-protocol -p tackly-client -p tackly-sync

# Cucumber acceptance tests: three real app windows (Patrick, Mona, Mara) are
# opened per scenario and clicked through, against a real server. Windows pop
# up and move on their own while this runs; let it finish.
acceptance:
    {{shell}} cargo test --locked -p tackly-acceptance --test acceptance

# Compile the app's styles (Tailwind) into crates/app/src/style.css. Run after
# changing class names; CI checks the result is up to date.
css:
    {{shell}} tailwindcss --input crates/app/tailwind.css --output crates/app/src/style.css --minify

# Clippy on everything. Unwrap and expect are denied (see Cargo.toml).
lint:
    {{shell}} cargo clippy --workspace --all-targets --locked -- -D warnings

# Debug APK for Android (needs the Android SDK and NDK; the shell has the rest).
android-build:
    {{shell}} {{scripts}}/android.sh build

# Build, install and start the app on the running emulator or device.
android:
    {{shell}} {{scripts}}/android.sh run

# The Playwright-on-Android suite: real taps and keyboard in an emulator.
android-test:
    {{shell}} {{scripts}}/android.sh test
