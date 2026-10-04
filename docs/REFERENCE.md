# Reference

[Quick start](../README.md)

## Build prerequisites

Requires Rust 1.93+, Cargo, and a COSMIC Wayland session. On Pop!_OS/Ubuntu:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libfontconfig1-dev fonts-dejavu-core
```

The applet uses pinned [libcosmic](https://github.com/pop-os/libcosmic) with software rendering. Keep `Cargo.lock` for reproducible builds. Without `--features applet`, core/helper builds skip compiling libcosmic.

## Install and uninstall

The installer defaults to `~/.local` and installs two binaries, a desktop entry, and an icon. It does not change panel configuration, add autostart entries, or enable reminders. Existing settings are preserved.

```sh
./scripts/install.sh
# Optional absolute prefix:
./scripts/install.sh /path/to/prefix

# Remove the applet from the dock/panel first; settings are kept.
./scripts/uninstall.sh
```

Uninstalling also leaves `~/.naal-focus/` activity history untouched. Both scripts accept the same optional prefix. After updating, remove/re-add the applet or restart the panel/dock to load the new binary; stopping an individual applet process may not relaunch it. Reopen COSMIC Settings if its applet list needs refreshing.

## Settings and controls

Settings live at `$XDG_CONFIG_HOME/naal-focus/settings.json`, falling back to `~/.config/naal-focus/settings.json`. Saves are atomic. Invalid, oversized, or unsupported-version files remain untouched until explicit **Save settings** replaces them.

| Setting | Default | Range |
|---|---:|---|
| Recurring reminders | Paused | Enabled/paused |
| Interval | 300 seconds | 15–86400; greater than duration |
| Duration | 5 seconds | 1–30 seconds |
| Typing time limit | 120 seconds | 30–600 seconds; separate from duration |
| Text size | 36 logical pixels | 22–72 |
| Idle threshold | 120 seconds | 30–3600 |
| Quotes | Four reminders | 1–100; ≤500 characters each, including line breaks |

- **Save settings** applies edited fields together, cancels pending/visible reminders, and restarts the interval. Other popups refresh saved values without overwriting unsaved edits.
- **Recurring reminders** persists immediately.
- **Snooze 15 min** is temporary. A full interval follows the snooze; it does not survive closing every instance.
- **Resume** enables paused reminders or clears a snooze. It is disabled when neither action is needed.
- **Show now** waits two seconds before displaying a one-shot reminder. It works while recurrence is paused, but cannot bypass safety guards.

Every monitor popup and `naal-focus --settings` shares one timer through session D-Bus. Closing a secondary instance leaves reminders running. If the owner closes, another instance takes over with the remaining snooze and a fresh interval. Closing all instances stops reminders and the helper.

## Activity reflections and history

The overlay has one shared reflection across all covered monitors. Its single-line textbox is ready for input without clicking; the quote remains prominent. No typing means no logging and no change to the normal dismissal countdown.

- First text input pauses the normal countdown, retaining its exact remaining duration. The underline below the field shows the fixed typing allowance remaining.
- **Enter** saves nonblank text (up to 2000 characters) through a background worker. After acknowledgment, the textbox disappears into a quiet rendering of the accepted text and the remaining normal countdown resumes—not a fresh duration.
- Click the accepted text to reopen it. Merely clicking, moving the caret, or selecting text does not pause the countdown. Typing, replacing, or deleting text does. Enter safely corrects the same record, preserving its original timestamp; it does not add another log.
- **Esc/background click** cancels a writing draft or correction and resumes the remaining countdown. A correction restores the last accepted text. Closing a reopened but unchanged editor leaves the countdown untouched. Otherwise Esc/background click dismisses all monitors as before. Field clicks never dismiss the overlay.
- The typing allowance starts at the first text change and never restarts during that reminder, even across save/cancel/reopen cycles. While writing, reaching the limit dismisses all outputs and discards unsaved text. After saving or canceling, the remaining normal countdown may finish even if the typing allowance expires; further changes cannot renew it.
- Empty submissions are rejected. Failed saves retain the draft and display an error while the safety deadline stays active. Clearing all text is not a save or cancellation.
- Cancellation is not Undo for a submission already initiated with Enter: an in-flight explicit save can complete. Unsaved drafts are never stored automatically.

History lives at `$HOME/.naal-focus/activity.jsonl`, independent of XDG configuration and install prefix. `HOME` must be absolute. The directory is private (0700) and the file is private (0600), owned by the current user; symlink/hardlink targets are refused. No cloud, accounts, application monitoring, raw key history, or writing-duration measurement is involved.

Each JSON line is a versioned record with an opaque identity, UTC Unix timestamp in milliseconds, and submitted text:

```json
{"version":1,"id":"0123456789abcdef0123456789abcdef","timestamp_unix_ms":1791100000000,"text":"Reviewing documentation."}
```

New entries append; corrections atomically replace the matching entry while preserving other records. Retries with the same identity/payload are idempotent. Earlier version-1 records without an ID remain readable and are preserved. Malformed, incomplete, oversized, or unsupported-version history is not silently truncated or overwritten: repair it before retrying. There is no history browser or dashboard yet.

Basic caret navigation, selection, and UTF-8/compose input are supported; clipboard integration and full IME support are not implemented. Very long single-line reflections scroll while editing and are shortened visually in the accepted-text view, without shortening the saved record.

## Quote format

- An unindented `- ` starts a new quote. A bare `-` starts an empty item, which needs following text.
- Following lines belong to that quote until the next marker. Unindented continuations work, but two-space indentation is clearer.
- One two-space continuation indent is removed on save. Thus `  - Literal bullet.` remains inside the quote rather than creating another one.
- Interior blank lines create paragraphs. Leading/trailing blank lines are ignored; empty quotes are rejected.
- Long lines wrap automatically. The entire quote must fit the 500-character limit; tabs and other control characters are disallowed.

This is plain text, not full Markdown: emphasis, links, headings, and nested-list styling are not interpreted. Existing JSON quote arrays need no migration. Unbulleted one-per-line lists can still be pasted; when bullet markers are present, text before the first marker is an error.

The helper's `--stdin` accepts one raw quote, including line breaks and literal bullets—not a bulk-editor list. Increase the visible duration for longer quotes.

## Development and checks

```sh
just build          # Debug applet and helper
just release        # Release binaries
just check          # Formatting, Clippy, unit tests, shell syntax
just check-native   # Isolated multi-client supervisor regression
just probe          # Read-only monitor/protocol/safety check
just settings       # Standalone controls; honors saved reminder preferences
```

`check-native` uses a private D-Bus session/runtime and never requests overlays or enables recurrence. `probe` creates no surfaces. Run `just` to list all recipes.

See the [testing guide](TESTING.md) for coverage, recorded verification results, and manual acceptance checks. **Manual overlays capture keyboard focus**; use a disposable editor document and save other work first. A manually launched helper has no independent parent watchdog; SIGSTOP pauses its own timeout too.

## Runtime safety

See [architecture and safety rationale](ARCHITECTURE.md) for supervisor election, compositor routing, and why these safeguards are required.

- One elected applet supervises the scheduler and helper; D-Bus ownership and a process lock prevent duplicate timers.
- Lock, idle, sleep, missing guard data, and stale guard data block reminders. Checks use logind, COSMIC's runtime lock marker, and Wayland `ext_idle_notifier_v1` version 2.
- Returning from an unsafe session or a scheduling gap starts a full interval. `CLOCK_BOOTTIME` includes suspended time; missed reminders are discarded.
- The helper uses the desktop `WAYLAND_DISPLAY`, ignoring the panel's nested `WAYLAND_SOCKET`. `NAAL_FOCUS_WAYLAND_DISPLAY` can override it.
- Compositor closure, output hotplug, countdown expiry, or typing-limit expiry while writing dismisses all surfaces. Escape/background click cancels an active draft first; without a draft it dismisses normally. The supervisor kills and reaps unsafe/stalled helpers, with an independent duration-plus-typing-limit-plus-eight-second watchdog and parent-death signaling. Rendering failures pause recurrence.

This is not a screen lock. Compositor shortcuts can still work; focus restoration is controlled by COSMIC. No inhibitors or system services are installed.

## Limitations

- Fractional scaling is rounded to an integer buffer scale.
- A single font is used, without comprehensive multilingual shaping/fallback. Complex scripts or missing glyphs may render incorrectly.
- Very dense quotes may not fit small screens. Each frame is capped at 32 million pixels per output and 64 million combined. Two reusable frames per output keep long writing sessions from accumulating an unbounded frame queue.
- Seat-topology changes require restarting the applet.
- No DND/fullscreen suppression. Manual settings-file edits take effect after restart.

## Source map

| Files | Responsibility |
|---|---|
| `src/applet.rs` | Native controls and popup |
| `src/config.rs` | Settings, quote parsing/formatting, persistence |
| `src/activity.rs`, `src/activity_log.rs` | Reflection editing/countdowns, private history and atomic corrections |
| `src/schedule.rs` | Scheduling, snooze, quote rotation |
| `src/engine.rs`, `src/guard.rs`, `src/process.rs` | Shared controls, safety, supervision |
| `src/overlay.rs`, `src/render.rs` | Overlay surfaces and text rendering |
| `data/`, `scripts/`, `tests/` | Packaging, assets, regression checks |
