#!/usr/bin/env bash
set -euo pipefail
prefix=${1:-"$HOME/.local"}
[[ $prefix = /* ]] || { printf 'Install prefix must be absolute.\n' >&2; exit 1; }
printf 'Remove the applet from COSMIC Settings first. Settings are preserved.\n'
rm -f -- "$prefix/bin/naal-focus" "$prefix/bin/naal-focus-overlay" \
    "$prefix/share/applications/io.github.rinaldo_rex.NaalFocus.desktop" \
    "$prefix/share/icons/hicolor/scalable/apps/naal-focus-symbolic.svg"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$prefix/share/applications" || true
fi
printf 'Uninstalled Naal Focus from %s.\n' "$prefix"
