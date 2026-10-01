# Naal Focus

A native **COSMIC dock/panel applet** that briefly dims every monitor with a mindfulness reminder. Based on the working Wayland proof of concept in `poc/`; this is not a browser mockup.

## Features

- Native themed icon, tooltip, and click-open settings popup.
- Persistent interval, duration, quote collection, text size, and idle threshold.
- Pause, snooze for 15 minutes, resume, and **Show now** (two-second preparation delay).
- Quotes rotate in order. The same quote appears on every connected monitor.
- One coordinated fullscreen overlay across all monitors, including panels/docks.
- **Escape, a mouse-button release, or the timeout dismisses every screen together.**
- Lock/idle/suspend safeguards, no catch-up bursts, singleton enforcement, supervised helper cleanup, and a separate parent-process watchdog.

**Recurring reminders default to paused.** Installation does not start anything, enable autostart, or modify panel configuration. Enable reminders explicitly in the popup.

> Reminders deliberately capture keyboard focus. Ordinary keys are consumed and never logged. This is an attention interruption, **not a screen lock**; compositor shortcuts can still work.

## Build

Rust **1.93+**, Cargo, a COSMIC Wayland session, and native build/font prerequisites are required. On Pop!_OS/Ubuntu, install missing prerequisites if necessary:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libfontconfig1-dev fonts-dejavu-core
```

The native UI uses pinned [libcosmic](https://github.com/pop-os/libcosmic) with software rendering; no GPU-specific development setup is needed. Cargo fetches its Git submodules and dependencies. Keep `Cargo.lock` for reproducibility.

```sh
cargo build --locked --features applet
cargo test --locked --features applet
cargo clippy --locked --features applet --all-targets -- -D warnings
cargo fmt --check

# Read-only: list monitors and inspect session/protocol safety. No overlay.
cargo run --locked --bin naal-focus-overlay -- --check

# Real standalone controls, useful before adding the panel applet.
./target/debug/naal-focus --settings
```

Without `--features applet`, the core/helper can be built and tested without compiling libcosmic. `just` lists equivalent recipes; its default only lists commands.

## Install into your user account

```sh
cargo build --locked --release --features applet
./scripts/install.sh              # default prefix: ~/.local
# Or: just install
```

The installer places two binaries, a desktop entry, and an icon under the chosen prefix. The desktop entry uses an absolute executable path and `X-HostWaylandDisplay=true`.

Then open **COSMIC Settings → Desktop → Dock or Panel → Applets**, find **Naal Focus**, and add it. Click the icon to configure it. No panel settings are edited by the installer. If the applet list is already open, close/reopen Settings to refresh it.

Only **one applet instance** owns the reminder timer. A duplicate instance reports that another applet is running instead of starting another timer. Close a standalone `--settings` instance before adding the applet to a panel. Removing the applet stops its supervised helper; adding it again honors the persisted enabled/paused setting and waits a full interval.

```sh
./scripts/uninstall.sh           # remove from the dock/panel first
```

Uninstall preserves settings. Scripts accept an optional absolute prefix, e.g. `./scripts/install.sh /tmp/naal-package-test`.

## Settings

Stored at `$XDG_CONFIG_HOME/naal-focus/settings.json`, or `~/.config/naal-focus/settings.json`. Settings are validated and saved using an atomic rename with file/directory synchronization. A malformed, oversized, or future-version file is not silently overwritten. Explicit **Save settings** can replace it with repaired settings; enabling alone cannot.

| Setting | Default | Valid range |
|---|---:|---|
| Recurring reminders | Paused | Enabled/paused |
| Interval | 300 seconds | 15–86400; must exceed duration |
| Visible duration | 5 seconds | 1–30 |
| Text size | 36 logical pixels | 22–72 |
| Idle threshold | 120 seconds | 30–3600 |
| Quotes | Four bundled reminders | 1–100 lines, ≤500 characters each |

Save applies the edited fields together. It restarts the interval and cancels any visible/pending reminder. The enable toggle persists immediately. Snooze is temporary, is not saved across restarts, and adds a full interval after the snooze ends. **Show now** is an explicit one-shot action that works while recurring reminders are paused, but never bypasses session safety.

## Safety and scheduling

- The applet owns the only recurring timer; the helper has **no loop mode**.
- The helper explicitly connects to the desktop socket identified by `WAYLAND_DISPLAY`, ignoring the panel's inherited `WAYLAND_SOCKET` and privileged socket. `NAAL_FOCUS_WAYLAND_DISPLAY` can override the desktop socket if needed; no display name is hardcoded.
- logind's graphical user session is checked for activity, lock, idle, and preparation for sleep. COSMIC's `cosmic-greeter-<session>.lock` runtime marker is also checked, since the current locker does not reliably update `LockedHint`.
- Wayland `ext_idle_notifier_v1` **version 2** tracks input inactivity independently of video-player idle inhibitors. Missing/stale guard data blocks reminders, including **Show now**.
- `CLOCK_BOOTTIME` includes time spent suspended. A scheduling gap or return from an unsafe session restarts the full interval; missed reminders are discarded.
- At most one helper runs. Escape/click/timeout, compositor closure, or output hotplug removes all its surfaces together. The parent kills an unsafe, overdue, or stalled helper and reaps it. Linux parent-death signaling terminates the helper if its launching supervisor thread dies.
- The helper has its own hard deadline, and the parent has an independent watchdog (duration + 8 seconds, allowing guard/configure startup). Rendering failure pauses recurrence rather than reopening a broken overlay repeatedly. A manually launched helper has only its internal watchdog; stopping its entire process with SIGSTOP also stops that watchdog.

No inhibitors are installed, no global shortcuts are blocked, and no system services are enabled. Previously focused-app restoration is controlled by COSMIC, not forced by this applet.

## Manual overlay tests — explicit opt-in

Use a disposable editor document, not an important terminal command:

```sh
# WARNING: all screens dim and keyboard focus is captured, after three seconds.
./target/release/naal-focus-overlay --once --all --delay-secs 3

# Test a particular output from --check, if needed.
./target/release/naal-focus-overlay --once --output HDMI-A-1 --delay-secs 3
```

Check timeout, Escape, click-release, readable text on each scale, blocked typing during the overlay, and focus restoration afterward. Also check hotplug, fullscreen applications, idle, lock/unlock, and suspend/resume. Do not trigger lock/suspend tests while doing unsaved work.

The read-only supervisor integration test exercises settings, snooze/resume, and duplicate exclusion, without requesting overlays or enabling recurrence. Close other Naal Focus instances first:

```sh
cargo test --locked --features applet --test supervisor -- --ignored --test-threads=1
```

## Verification and current limits

On 2026-10-01, in the target COSMIC session:

- Formatting, warnings-denied Clippy, and 14 unit tests passed; native debug/release binaries built.
- The read-only supervisor integration test passed: settings persistence/repair, duplicate exclusion, and snooze/resume, with no overlay or recurring timer enabled.
- User-local installation and an install/uninstall round trip under a prefix containing spaces passed; the installed desktop entry passed `desktop-file-validate`.
- The read-only probe resolved all three outputs and reported ready lock/idle guards.
- A five-second live reminder mapped **HDMI-A-1, DP-1, and eDP-1**; the user confirmed all screens dimmed with readable text. eDP-1 rendered at buffer scale 2.
- An automated Escape test dismissed all three surfaces together. Both tests exited successfully and left no helper running.
- The standalone native control window opened successfully.

**Not yet verified live:** click-release dismissal, blocked typing/focus restoration, actual dock/panel popup placement, recurring reminders, lock/suspend transitions, and hotplug. These are acceptance checks, not claims established by unit tests.

Rendering supports integer buffer scaling (fractional compositor scales use the rounded integer scale), bounded word/character wrapping, and text-size adjustment. It still uses fontdue with a single installed sans-serif font, not a full multilingual shaping/fallback engine; complex scripts and unsupported glyphs may not render correctly. Exceptionally dense long quotes on very small screens can still need a smaller text size. Per-output buffers are capped at 32 million pixels and combined buffers at 64 million; unsupported layouts fail safely. Seat-topology changes require restarting the applet. No animations, translated UI, DND/fullscreen suppression, or remote configuration watcher yet; manual file edits take effect after restart.

## Source map

| Path | Responsibility |
|---|---|
| `src/applet.rs` | Native COSMIC icon, controls, popup, and standalone view |
| `src/config.rs` | Versioned settings, validation, and atomic persistence |
| `src/schedule.rs` | Pure no-burst scheduling, snooze, and quote rotation |
| `src/engine.rs` | Single timer/helper supervisor and watchdog |
| `src/guard.rs` | logind/COSMIC lock and Wayland input-idle checks |
| `src/process.rs` | Desktop socket routing, singleton locks, child lifecycle |
| `src/overlay.rs`, `src/render.rs` | Coordinated layer-shell surfaces and text rendering |
| `data/`, `scripts/` | Quotes, icon, applet metadata, user-local installation |
| `tests/supervisor.rs` | Opt-in read-only native supervisor integration test |
| `poc/` | Original local POC reference, deliberately left untouched/ignored |
