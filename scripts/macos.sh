#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { printf 'This command requires macOS.\n' >&2; exit 1; }
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
action=${1:-bundle}
destination=${2:-"$root/target/macos"}
[[ $destination = /* ]] || { printf 'Destination must be absolute.\n' >&2; exit 1; }
name="Naal Focus.app"

check_existing() {
    [[ ! -L "$destination/$name" ]] || { printf 'Refusing a linked app bundle.\n' >&2; exit 1; }
    if [[ -e "$destination/$name" ]]; then
        identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$destination/$name/Contents/Info.plist")
        [[ $identifier == io.github.rinaldo-rex.NaalFocus ]] || { printf 'Destination contains a different app.\n' >&2; exit 1; }
    fi
    if pgrep -x naal-focus >/dev/null; then
        printf 'Quit Naal Focus from its menu bar first. Nothing changed.\n' >&2
        exit 1
    fi
}

case "$action" in
    uninstall)
        check_existing
        rm -rf -- "${destination:?}/${name:?}"
        printf 'Removed app; settings and history kept.\n'
        exit 0
        ;;
    bundle|install) ;;
    *) printf 'Usage: macos.sh bundle|install|uninstall /absolute/destination\n' >&2; exit 1 ;;
esac

binary="$root/target/release/naal-focus"
[[ -x "$binary" ]] || { printf 'Build first: just release\n' >&2; exit 1; }
mkdir -p "$destination"
work=$(mktemp -d "$destination/.naal-focus.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
app="$work/$name"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$binary" "$app/Contents/MacOS/naal-focus"
install -m 644 "$root/LICENSE" "$app/Contents/Resources/LICENSE"
version=$("$binary" --version)
version=${version##* }
template=$(< "$root/data/macos/Info.plist")
printf '%s\n' "${template//@VERSION@/$version}" > "$app/Contents/Info.plist"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
# Ad-hoc integrity signing needs no Apple account and is NOT notarization.
/usr/bin/codesign --force --sign - "$app"
/usr/bin/codesign --verify --strict "$app"
if [[ $action == bundle ]]; then
    /usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$destination/Naal-Focus-macOS.zip"
    (cd -- "$destination" && shasum -a 256 Naal-Focus-macOS.zip > Naal-Focus-macOS.zip.sha256)
    install -m 755 "$root/scripts/install-macos.sh" "$destination/install-macos.sh"
    printf 'Experimental test build: %s/Naal-Focus-macOS.zip\n' "$destination"
    printf 'Ad-hoc signed only; macOS may require explicit approval to open.\n'
else
    check_existing
    # Stage first, then replace only an identified, stopped Naal Focus app.
    if [[ -e "$destination/$name" ]]; then
        mv -- "$destination/$name" "$work/previous.app"
    fi
    if ! mv -- "$app" "$destination/$name"; then
        [[ ! -e "$work/previous.app" ]] || mv -- "$work/previous.app" "$destination/$name"
        exit 1
    fi
    printf 'Installed %s/%s. Not launched; no login item added.\n' "$destination" "$name"
fi
