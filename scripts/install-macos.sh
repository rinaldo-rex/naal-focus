#!/bin/sh
set -eu

fail() {
    printf 'Naal Focus: %s\n' "$*" >&2
    exit 1
}

check_destination() {
    [ ! -L "$target" ] || fail 'Refusing to replace a linked app bundle.'
    if [ -e "$target" ]; then
        [ -d "$target" ] || fail 'The destination contains a different file.'
        identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$target/Contents/Info.plist" 2>/dev/null) || fail 'The destination is not a recognized app.'
        [ "$identifier" = io.github.rinaldo-rex.NaalFocus ] || fail 'The destination contains a different app.'
    fi
    if pgrep -x naal-focus >/dev/null 2>&1; then
        fail 'Quit Naal Focus from its menu bar before installing.'
    else
        status=$?
        [ "$status" -eq 1 ] || fail 'Could not check whether Naal Focus is running.'
    fi
}

cleanup() {
    result=$?
    trap - 0 HUP INT TERM
    if [ -n "$work" ]; then
        if [ -d "$work/previous.app" ] && [ ! -e "$target" ] && [ ! -L "$target" ]; then
            if ! mv -- "$work/previous.app" "$target"; then
                printf 'Restore the previous app from %s/previous.app\n' "$work" >&2
                rmdir "$lock" || :
                exit 1
            fi
        fi
        rm -rf -- "$work"
    fi
    if [ "$lock_owned" = yes ]; then
        rmdir "$lock"
    fi
    exit "$result"
}

main() {
    if [ "$#" -eq 1 ] && { [ "$1" = --help ] || [ "$1" = -h ]; }; then
        printf 'Usage: install-macos.sh [/absolute/install-directory]\nDefault: ~/Applications. Apple Silicon, macOS 14+. Does not launch the app.\n'
        return
    fi
    [ "$#" -le 1 ] || fail 'Usage: install-macos.sh [/absolute/install-directory]'
    [ "$(uname -s)" = Darwin ] || fail 'This installer requires macOS.'
    [ "$(uname -m)" = arm64 ] || fail 'This preview requires Apple Silicon; run from a native Mac terminal.'
    major=$(sw_vers -productVersion)
    major=${major%%.*}
    case "$major" in ''|*[!0-9]*) fail 'Could not determine the macOS version.' ;; esac
    [ "$major" -ge 14 ] || fail 'This preview requires macOS 14 or later.'
    [ "$(id -u)" -ne 0 ] || fail 'Run without sudo; the app installs for your user.'

    destination=${1:-"${HOME:?HOME must be set}/Applications"}
    case "$destination" in /*) ;; *) fail 'The install directory must be an absolute path.' ;; esac
    target="$destination/Naal Focus.app"
    check_destination
    umask 077
    mkdir -p "$destination"
    work=
    lock="$destination/.naal-focus-install.lock"
    lock_owned=no
    trap cleanup 0
    trap 'exit 1' HUP INT TERM
    mkdir "$lock" 2>/dev/null || fail "Another installation may be running: $lock"
    lock_owned=yes
    # Stage on the destination filesystem so replacement is an atomic rename.
    work=$(mktemp -d "$destination/.naal-focus.XXXXXX")
    base=https://github.com/rinaldo-rex/naal-focus/releases/download/v0.9.0-macos-preview.1
    archive=Naal-Focus-macOS.zip
    printf 'Downloading the Apple Silicon preview…\n'
    for asset in "$archive" "$archive.sha256"; do
        curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
            --tlsv1.2 --retry 3 --connect-timeout 15 --max-time 180 \
            --output "$work/$asset" "$base/$asset" || fail 'Download failed. The repository and preview release must be public.'
    done

    expected=$(awk '
        NF { count++; if (NF != 2 || $2 != "Naal-Focus-macOS.zip") bad = 1; digest = $1 }
        END { if (count != 1 || bad) exit 1; print digest }
    ' "$work/$archive.sha256") || fail 'Invalid checksum file.'
    case "$expected" in ''|*[!0-9a-f]*) fail 'Invalid SHA-256 checksum.' ;; esac
    [ "${#expected}" -eq 64 ] || fail 'Invalid SHA-256 checksum.'
    actual=$(shasum -a 256 "$work/$archive")
    actual=${actual%% *}
    [ "$actual" = "$expected" ] || fail 'Checksum mismatch. The existing app was not changed.'

    unzip -Z1 "$work/$archive" > "$work/entries" || fail 'Invalid app archive.'
    awk '
        /^\// || /\\/ || /(^|\/)\.\.?($|\/)/ { bad = 1 }
        $0 != "__MACOSX/" && $0 !~ /^Naal Focus[.]app(\/|$)/ &&
            $0 !~ /^__MACOSX\/(\._Naal Focus[.]app|Naal Focus[.]app)(\/|$)/ { bad = 1 }
        END { exit bad }
    ' "$work/entries" || fail 'Unsafe paths in the app archive.'
    unzip -Z -l "$work/$archive" > "$work/permissions" || fail 'Invalid app archive.'
    awk 'substr($1, 1, 1) ~ /[bclps]/ { exit 1 }' "$work/permissions" || fail 'Linked or special files in the app archive.'
    ditto -x -k "$work/$archive" "$work/unpacked" || fail 'Could not extract the app.'
    app="$work/unpacked/Naal Focus.app"
    [ -d "$app" ] && [ ! -L "$app" ] || fail 'The archive does not contain Naal Focus.app.'
    identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null) || fail 'The downloaded app has no valid identity.'
    [ "$identifier" = io.github.rinaldo-rex.NaalFocus ] || fail 'The download contains a different app.'
    [ -x "$app/Contents/MacOS/naal-focus" ] || fail 'The app executable is missing.'
    codesign --verify --deep --strict "$app" || fail 'App integrity verification failed.'

    check_destination
    if [ -e "$target" ]; then
        mv -- "$target" "$work/previous.app"
    fi
    mv -- "$app" "$target" || fail 'Could not install the app; restoring the previous copy.'
    printf 'Installed %s\nNot launched; settings kept. No login item added.\n' "$target"
    printf 'This preview is ad-hoc signed, not notarized. Approve it in macOS only if you trust it.\n'
}

# Keeping all work inside main avoids acting on a partially downloaded script.
main "$@"
