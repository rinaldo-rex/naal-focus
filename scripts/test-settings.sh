#!/usr/bin/env bash
# Opens only an isolated settings window; never enables reminders or shows overlays.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command=(cargo test --locked --features applet --test settings_window -- --ignored --test-threads=1)
if [[ ${1:-} == capture && $# == 4 ]]; then
    command=(cargo run --locked --features applet --example settings-preview -- "$(realpath -m -- "$2")" "$3" "$4")
elif [[ $# != 0 ]]; then
    echo "Usage: $0 [capture OUTPUT WIDTH HEIGHT]" >&2
    exit 2
fi
display=${NAAL_FOCUS_WAYLAND_DISPLAY:-${WAYLAND_DISPLAY:?Run inside a Wayland session}}
if [[ $display != /* ]]; then display="${XDG_RUNTIME_DIR:?}/$display"; fi
runtime=$(mktemp -d /tmp/naal-settings-runtime.XXXXXX)
trap 'rm -rf -- "$runtime"' EXIT
# No service directories: do not auto-start desktop portals/keyrings or mount
# document/GVfs filesystems in the temporary runtime during this UI test.
printf '%s\n' '<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>' > "$runtime/session.conf"
mkdir -p "$runtime/home" "$runtime/config" "$runtime/cache" "$runtime/data"
# Keep Cargo's normal cache/toolchain while isolating all desktop-service data.
cargo_home=${CARGO_HOME:-$HOME/.cargo}
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
cd "$root"
HOME="$runtime/home" CARGO_HOME="$cargo_home" RUSTUP_HOME="$rustup_home" \
    XDG_CONFIG_HOME="$runtime/config" XDG_CACHE_HOME="$runtime/cache" \
    XDG_DATA_HOME="$runtime/data" XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY="$display" \
    NAAL_FOCUS_WAYLAND_DISPLAY="$display" \
    dbus-run-session --config-file="$runtime/session.conf" -- \
    "${command[@]}"
