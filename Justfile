install-prefix := if os() == "macos" { env_var("HOME") + "/Applications" } else { env_var("HOME") + "/.local" }

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
    bash -n scripts/*.sh

# Offline guide/dashboard checks in a separate, temporary headless Chrome.
check-web:
    pnpm test

check-docs:
    pnpm test:guide

check-dashboard:
    pnpm test:dashboard

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

# Offscreen composed reminders with fixed demo history at three desktop sizes.
preview-overlay output="/tmp/naal-overlay-native":
    TZ=UTC0 cargo run --locked --example overlay-preview -- "{{output}}"

# Six offline HTML fixtures; never reads saved history or starts a browser.
preview-dashboard output="/tmp/naal-dashboard-demo":
    TZ=UTC0 cargo run --locked --example dashboard-preview -- "{{output}}"

# Read-only: creates no overlay and captures no focus.
probe:
    cargo run --locked {{ if os() == "macos" { "--features applet --bin naal-focus" } else { "--bin naal-focus-overlay" } }} -- --check

# Open the desktop settings window; no reminder opt-in.
settings: build
    ./target/debug/naal-focus --settings

# WARNING: all monitors dim; keyboard focus is captured briefly. Explicit opt-in.
once: build
    ./target/debug/naal-focus-overlay --once --all --delay-secs 3

bundle-macos output=(justfile_directory() + "/target/macos"): release
    ./scripts/macos.sh bundle "{{output}}"

install prefix=install-prefix: release
    {{ if os() == "macos" { "./scripts/macos.sh install" } else { "./scripts/install.sh" } }} "{{prefix}}"

# Remove the COSMIC applet or quit the Mac app first; saved settings are kept.
uninstall prefix=install-prefix:
    {{ if os() == "macos" { "./scripts/macos.sh uninstall" } else { "./scripts/uninstall.sh" } }} "{{prefix}}"
