#!/usr/bin/env bash
# Read-only native regression; does not touch the installed applet or show overlays.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
display=${NAAL_FOCUS_WAYLAND_DISPLAY:-${WAYLAND_DISPLAY:?Run inside a Wayland session}}
if [[ $display != /* ]]; then display="${XDG_RUNTIME_DIR:?}/$display"; fi
runtime=$(mktemp -d /tmp/naal-supervisor-runtime.XXXXXX)
trap 'rm -rf -- "$runtime"' EXIT
cd "$root"
XDG_RUNTIME_DIR="$runtime" NAAL_FOCUS_WAYLAND_DISPLAY="$display" \
    dbus-run-session -- cargo test --locked --features applet --test supervisor -- --ignored --test-threads=1
