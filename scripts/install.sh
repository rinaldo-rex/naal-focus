#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
prefix=${1:-"$HOME/.local"}
[[ $prefix = /* ]] || { printf 'Install prefix must be absolute.\n' >&2; exit 1; }
for binary in naal-focus naal-focus-overlay; do
    [[ -x "$root/target/release/$binary" ]] || {
        printf 'Build first: cargo build --locked --release --features applet\n' >&2
        exit 1
    }
done
mkdir -p "$prefix/bin" "$prefix/share/applications" "$prefix/share/icons/hicolor/scalable/apps"
for binary in naal-focus naal-focus-overlay; do
    temp=$(mktemp "$prefix/bin/.$binary.XXXXXX")
    install -m 755 "$root/target/release/$binary" "$temp"
    mv -f -- "$temp" "$prefix/bin/$binary"
done
executable="$prefix/bin/naal-focus"
# Desktop Exec quoting rules differ from shell quoting.
executable=${executable//\\/\\\\}
executable=${executable//\"/\\\"}
executable=${executable//\$/\\\$}
executable=${executable//\`/\\\`}
entry=$(< "$root/data/io.github.rinaldo_rex.NaalFocus.desktop")
entry=${entry//@EXEC@/\"$executable\"}
printf '%s\n' "$entry" > "$prefix/share/applications/io.github.rinaldo_rex.NaalFocus.desktop"
chmod 644 "$prefix/share/applications/io.github.rinaldo_rex.NaalFocus.desktop"
install -m 644 "$root/data/naal-focus-symbolic.svg" "$prefix/share/icons/hicolor/scalable/apps/naal-focus-symbolic.svg"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$prefix/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null && [[ -f "$prefix/share/icons/hicolor/index.theme" ]]; then
    gtk-update-icon-cache -f -t "$prefix/share/icons/hicolor" || true
fi
printf 'Installed under %s. No panel settings or autostart entries changed.\n' "$prefix"
printf 'Add Naal Focus through COSMIC Settings → Desktop → Dock/Panel → Applets.\n'
printf 'New settings default to paused; existing reminder preferences are preserved.\n'
