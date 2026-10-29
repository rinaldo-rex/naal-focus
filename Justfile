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
    bash -n scripts/install.sh scripts/uninstall.sh scripts/test-supervisor.sh scripts/test-settings.sh

# Isolated session/runtime: three control clients, no overlays or recurring timer.
check-native:
    ./scripts/test-supervisor.sh

# Opens one isolated settings window; recurrence stays paused, no overlays.
check-settings:
    ./scripts/test-settings.sh

# Demo-only native settings window; no supervisor, reminders, or private data.
preview-settings output="/tmp/naal-settings-native" width="1000" height="660":
    ./scripts/test-settings.sh capture "{{output}}" "{{width}}" "{{height}}"

# Offscreen PPM snapshots/timings; no Wayland connection or keyboard focus.
preview-breathing output="/tmp/naal-breathing-native":
    cargo run --locked --release --example breathing-preview -- "{{output}}"

# Read-only: creates no overlay and captures no focus.
probe:
    cargo run --locked --bin naal-focus-overlay -- --check

# Open/focus the dedicated COSMIC settings window; no reminder opt-in.
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
