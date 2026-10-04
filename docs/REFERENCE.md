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

Both scripts accept the same optional prefix. After updating, remove/re-add the applet or restart the panel/dock to load the new binary; stopping an individual applet process may not relaunch it. Reopen COSMIC Settings if its applet list needs refreshing.

## Settings and controls

Settings live at `$XDG_CONFIG_HOME/naal-focus/settings.json`, falling back to `~/.config/naal-focus/settings.json`. Saves are atomic. Invalid, oversized, or unsupported-version files remain untouched until explicit **Save settings** replaces them.

| Setting | Default | Range |
|---|---:|---|
| Recurring reminders | Paused | Enabled/paused |
| Interval | 300 seconds | 15–86400; greater than duration |
| Duration | 5 seconds | 1–30 seconds |
| Text size | 36 logical pixels | 22–72 |
| Idle threshold | 120 seconds | 30–3600 |
| Quotes | Four reminders | 1–100; ≤500 characters each, including line breaks |

- **Save settings** applies edited fields together, cancels pending/visible reminders, and restarts the interval. Other popups refresh saved values without overwriting unsaved edits.
- **Recurring reminders** persists immediately.
- **Snooze 15 min** is temporary. A full interval follows the snooze; it does not survive closing every instance.
- **Resume** enables paused reminders or clears a snooze. It is disabled when neither action is needed.
- **Show now** waits two seconds before displaying a one-shot reminder. It works while recurrence is paused, but cannot bypass safety guards.

Every monitor popup and `naal-focus --settings` shares one timer through session D-Bus. Closing a secondary instance leaves reminders running. If the owner closes, another instance takes over with the remaining snooze and a fresh interval. Closing all instances stops reminders and the helper.

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

### Manual overlay tests

**These capture keyboard focus.** Use a disposable editor document, not an important terminal command.

```sh
./target/release/naal-focus-overlay --once --all --delay-secs 3
# Replace OUTPUT_NAME with a monitor listed by the probe:
./target/release/naal-focus-overlay --once --output OUTPUT_NAME --delay-secs 3
```

Check timeout, Escape, click-release, multiline layout, monitor scaling, typing suppression, focus restoration, hotplug, and lock/suspend transitions. Do not lock or suspend while doing unsaved work. A manually launched helper has no independent parent watchdog; SIGSTOP pauses its own timeout too.

## Runtime safety

See [architecture and safety rationale](ARCHITECTURE.md) for supervisor election, compositor routing, and why these safeguards are required.

- One elected applet supervises the scheduler and helper; D-Bus ownership and a process lock prevent duplicate timers.
- Lock, idle, sleep, missing guard data, and stale guard data block reminders. Checks use logind, COSMIC's runtime lock marker, and Wayland `ext_idle_notifier_v1` version 2.
- Returning from an unsafe session or a scheduling gap starts a full interval. `CLOCK_BOOTTIME` includes suspended time; missed reminders are discarded.
- The helper uses the desktop `WAYLAND_DISPLAY`, ignoring the panel's nested `WAYLAND_SOCKET`. `NAAL_FOCUS_WAYLAND_DISPLAY` can override it.
- Escape, mouse-button release, timeout, compositor closure, or output hotplug dismisses all surfaces. The supervisor kills and reaps unsafe/stalled helpers, with an independent duration-plus-eight-second watchdog and parent-death signaling. Rendering failures pause recurrence.

This is not a screen lock. Compositor shortcuts can still work; focus restoration is controlled by COSMIC. No inhibitors or system services are installed.

## Limitations

- Fractional scaling is rounded to an integer buffer scale.
- A single font is used, without comprehensive multilingual shaping/fallback. Complex scripts or missing glyphs may render incorrectly.
- Very dense quotes may not fit small screens. Buffers are capped at 32 million pixels per output and 64 million combined.
- Seat-topology changes require restarting the applet.
- No DND/fullscreen suppression. Manual settings-file edits take effect after restart.

## Source map

| Files | Responsibility |
|---|---|
| `src/applet.rs` | Native controls and popup |
| `src/config.rs` | Settings, quote parsing/formatting, persistence |
| `src/schedule.rs` | Scheduling, snooze, quote rotation |
| `src/engine.rs`, `src/guard.rs`, `src/process.rs` | Shared controls, safety, supervision |
| `src/overlay.rs`, `src/render.rs` | Overlay surfaces and text rendering |
| `data/`, `scripts/`, `tests/` | Packaging, assets, regression checks |
