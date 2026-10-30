#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { printf 'Installer checks require macOS.\n' >&2; exit 1; }
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
assets=${1:-"$root/target/macos"}
installer="$assets/install-macos.sh"
archive=Naal-Focus-macOS.zip
cmp "$root/scripts/install-macos.sh" "$installer"
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
mkdir -p "$work/bin" "$work/home/Library/Application Support/Naal Focus"
settings="$work/home/Library/Application Support/Naal Focus/settings.json"
printf 'keep existing preferences\n' > "$settings"
export HOME="$work/home" NAAL_TEST_ASSETS="$assets"
export PATH="$work/bin:$PATH"

cat > "$work/bin/curl" <<'SH'
#!/bin/sh
set -eu
output= url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        https://github.com/rinaldo-rex/naal-focus/releases/download/v0.9.0-macos-preview.1/*) url=$1; shift ;;
        *) shift ;;
    esac
done
[ -n "$output" ] && [ -n "$url" ] || exit 2
cp "$NAAL_TEST_ASSETS/${url##*/}" "$output"
SH
cat > "$work/bin/pgrep" <<'SH'
#!/bin/sh
exit "${NAAL_TEST_PGREP_STATUS:-1}"
SH
cat > "$work/bin/mv" <<'SH'
#!/bin/sh
if [ "${NAAL_TEST_FAIL_RENAME:-0}" = 1 ]; then
    for argument in "$@"; do
        case "$argument" in */unpacked/Naal\ Focus.app) exit 1 ;; esac
    done
fi
exec /bin/mv "$@"
SH
chmod +x "$work/bin/"*

checks=0
pass() {
    checks=$((checks + 1))
    printf 'PASS %s\n' "$1"
}
expect_failure() {
    local name=$1
    shift
    if "$@" > "$work/failure.log" 2>&1; then
        printf 'FAIL %s: unexpectedly succeeded\n' "$name" >&2
        exit 1
    fi
    pass "$name"
}
assert_preserved() {
    [[ -f "$target/Contents/Resources/keep-old-app" ]]
    [[ ! -e "$destination/.naal-focus-install.lock" ]]
    [[ $(find "$destination" -name '.naal-focus.*' -print | wc -l | tr -d ' ') == 0 ]]
}
checksum() {
    (cd -- "$1" && shasum -a 256 "$archive" > "$archive.sha256")
}

sh "$installer" --help > /dev/null
pass 'help without installation'
awk '!/^main "\$@"$/' "$installer" | sh -s -- "$work/truncated"
[[ ! -e "$work/truncated" ]]
pass 'truncated stream does not install anything'
expect_failure 'relative destination' sh "$installer" relative/path

# Pipe through sh exactly as in the public curl command, without any network.
destination="$work/Applications with spaces"
target="$destination/Naal Focus.app"
cat "$installer" | sh -s -- "$destination"
[[ -x "$target/Contents/MacOS/naal-focus" ]]
cmp "$root/LICENSE" "$target/Contents/Resources/LICENSE"
codesign --verify --strict "$target"
[[ ! -e "$destination/.naal-focus-install.lock" ]]
pass 'streamed installation into a path containing spaces'

printf 'old app\n' > "$target/Contents/Resources/keep-old-app"
sh "$installer" "$destination"
[[ ! -e "$target/Contents/Resources/keep-old-app" ]]
pass 'replace an identified, stopped app'
printf 'old app\n' > "$target/Contents/Resources/keep-old-app"

expect_failure 'running app' env NAAL_TEST_PGREP_STATUS=0 sh "$installer" "$destination"
assert_preserved
expect_failure 'unknown process state' env NAAL_TEST_PGREP_STATUS=2 sh "$installer" "$destination"
assert_preserved
expect_failure 'rename failure' env NAAL_TEST_FAIL_RENAME=1 sh "$installer" "$destination"
assert_preserved

mkdir "$destination/.naal-focus-install.lock"
expect_failure 'concurrent installer' sh "$installer" "$destination"
[[ -d "$destination/.naal-focus-install.lock" ]]
rmdir "$destination/.naal-focus-install.lock"
assert_preserved

mkdir "$work/linked"
ln -s "$target" "$work/linked/Naal Focus.app"
expect_failure 'linked destination app' sh "$installer" "$work/linked"
[[ -L "$work/linked/Naal Focus.app" ]]
assert_preserved
mkdir -p "$work/other/Naal Focus.app/Contents"
cp "$target/Contents/Info.plist" "$work/other/Naal Focus.app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier other.app' "$work/other/Naal Focus.app/Contents/Info.plist"
expect_failure 'unrelated destination app' sh "$installer" "$work/other"

mkdir "$work/missing"
expect_failure 'missing release asset' env NAAL_TEST_ASSETS="$work/missing" sh "$installer" "$destination"
assert_preserved

mkdir "$work/bad"
cp "$assets/$archive" "$work/bad/$archive"
printf '%064d  %s\n' 0 "$archive" > "$work/bad/$archive.sha256"
expect_failure 'checksum mismatch' env NAAL_TEST_ASSETS="$work/bad" sh "$installer" "$destination"
assert_preserved
printf '%064d  ../../outside\n' 0 > "$work/bad/$archive.sha256"
expect_failure 'unsafe checksum filename' env NAAL_TEST_ASSETS="$work/bad" sh "$installer" "$destination"
assert_preserved

mkdir "$work/unsafe" "$work/zip-input"
printf 'outside\n' > "$work/outside"
(cd -- "$work/zip-input" && zip -q "$work/unsafe/$archive" ../outside)
checksum "$work/unsafe"
expect_failure 'archive path traversal' env NAAL_TEST_ASSETS="$work/unsafe" sh "$installer" "$destination"
assert_preserved

mkdir "$work/invalid"
ditto -x -k "$assets/$archive" "$work/invalid"
ln -s "$work/outside" "$work/invalid/Naal Focus.app/Contents/Resources/link"
(cd -- "$work/invalid" && zip -qry "$archive" 'Naal Focus.app')
checksum "$work/invalid"
expect_failure 'archive symlink' env NAAL_TEST_ASSETS="$work/invalid" sh "$installer" "$destination"
assert_preserved
rm "$work/invalid/Naal Focus.app/Contents/Resources/link" "$work/invalid/$archive"
printf 'changed resource\n' >> "$work/invalid/Naal Focus.app/Contents/Resources/LICENSE"
ditto -c -k --keepParent "$work/invalid/Naal Focus.app" "$work/invalid/$archive"
checksum "$work/invalid"
expect_failure 'invalid app signature' env NAAL_TEST_ASSETS="$work/invalid" sh "$installer" "$destination"
assert_preserved
rm "$work/invalid/$archive"
/usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier other.app' "$work/invalid/Naal Focus.app/Contents/Info.plist"
ditto -c -k --keepParent "$work/invalid/Naal Focus.app" "$work/invalid/$archive"
checksum "$work/invalid"
expect_failure 'unrelated downloaded app' env NAAL_TEST_ASSETS="$work/invalid" sh "$installer" "$destination"
assert_preserved

printf 'keep existing preferences\n' | cmp - "$settings"
[[ $(< "$work/outside") == outside ]]
pass 'settings and files outside the installation remain unchanged'
printf '%s offline installer checks passed; no app launched.\n' "$checks"
