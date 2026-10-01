default:
    @just --list

build:
    cargo build --locked --features applet

release:
    cargo build --locked --release --features applet

check:
    cargo fmt --check
    cargo clippy --locked --features applet --all-targets -- -D warnings
    cargo test --locked --features applet
    bash -n scripts/install.sh scripts/uninstall.sh

# Read-only: creates no overlay and captures no focus.
probe:
    cargo run --locked --bin naal-focus-overlay -- --check

# A real, standalone COSMIC control window; recurring reminders default to paused.
settings: build
    ./target/debug/naal-focus --settings

# WARNING: all monitors dim; keyboard focus is captured briefly. Explicit opt-in.
once: build
    ./target/debug/naal-focus-overlay --once --all --delay-secs 3

install prefix=(env_var("HOME") + "/.local"): release
    ./scripts/install.sh "{{prefix}}"

# Remove from your dock/panel first; saved settings are kept.
uninstall prefix=(env_var("HOME") + "/.local"):
    ./scripts/uninstall.sh "{{prefix}}"
