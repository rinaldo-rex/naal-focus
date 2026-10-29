# Testing

## Checks

```sh
just check          # Formatting, Clippy, unit tests, shell syntax
cargo test --locked # Core/helper tests without the applet UI library
just probe          # Read-only desktop, output, and safety check
```

Parallel runs can occasionally report `Activity history is busy`. Retry with `RUST_TEST_THREADS=1 just check`.

The probe opens no reminder and collects no window titles. Read its printed guard status as well as its exit code.

## Native checks

```sh
just check-native
just check-settings
```

These require a COSMIC Wayland session and `dbus-run-session`. They isolate settings/history and the session bus, leave the installed applet untouched, and never enable reminders or tracking. `check-settings` opens a real settings window. Normal checks skip these tests.

## Screenshots

```sh
just preview-breathing
just preview-settings
```

Breathing previews are offscreen; settings previews open a demo capture window. Both write PPM images under `/tmp` without reading personal history.

## Manual checks

**Only with explicit consent:** save other work, use a disposable editor document, and isolate settings and history in temporary directories. Pause recurring reminders first to avoid collisions.

Compare against the [app guide](html/index.html):

- All monitors: synchronized breathing/content, desktop window sizes and scaling, and safe dismissal on output hotplug.
- Input: no typing leaks through; saving, corrections, history, suggestions, Escape, and background clicks work as described.
- Timing: untouched reminders close on time, entrance input is ignored, and cancel/reopen cannot renew the typing allowance.
- Recovery: focus returns after dismissal; idle, lock, and sleep block reminders; the parent terminates a stalled supervised helper.
- Settings: one reusable window, keyboard access, retained drafts, visible validation, and closing leaves the applet running.
- History: tracking requires consent, stops on its own switch, excludes unsafe periods, and retains only today. The dashboard opens and its download works offline.

Do not freeze a directly launched helper with SIGSTOP: it has no independent parent watchdog to recover focus. Test stalled-helper recovery through a supervised setup with a separate recovery terminal.

Record the revision, desktop/toolchain versions, outputs/scales, results, and untested cases in the PR. Passing automated checks or a probe is not live acceptance.
