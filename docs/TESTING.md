# Testing

## Checks

```sh
just check          # Formatting, Clippy, unit tests, shell syntax
cargo test --locked # Core/helper tests without the applet UI library
just probe          # Read-only desktop, output, and safety check
```

The probe opens no reminder and collects no window titles. Read its printed guard status as well as its exit code.

## Offline browser checks

Install Node.js 22+, pnpm, and Google Chrome, then run:

```sh
pnpm install --frozen-lockfile
just check-web
# Or: just check-docs / just check-dashboard
```

These separate checks validate guide links, anchors, image dimensions, and desktop layouts, then exercise the generated dashboard and its offline HTML download. They use fixed demo data, a temporary headless browser profile, and no network. Your open browser and saved history stay untouched.

The desktop matrix covers 1000×660, 1920×1080, and 1707×960 at 2× scaling. Screenshots, downloaded HTML, and failure traces go under `target/browser-tests/`; generated demo pages live under `target/browser-fixtures/`. These artifacts are for review, not cross-machine pixel baselines.

## Native checks

```sh
just check-native
just check-settings
```

These require a COSMIC Wayland session and `dbus-run-session`. They isolate settings/history and the session bus, leave the installed applet untouched, and never enable reminders or tracking. `check-settings` opens a real settings window. Normal checks skip these tests.

## Screenshots

```sh
just preview-breathing
just preview-overlay
just preview-dashboard
just preview-settings
```

Breathing and composed-reminder previews write offscreen PPM images. The overlay preview covers six states at three desktop sizes, including history, suggestions, saved text, charts, and reflections off. The dashboard preview writes six standalone HTML fixtures. Settings previews open a demo capture window. All default to `/tmp` and never read personal history.

The overlay/dashboard recipes fix the time zone to UTC and use a fixed date; set `TZ=UTC0` when running their Cargo examples directly. Desktop fonts can affect rendered pixels. Review captures before replacing guide images; keep their documented dimensions in sync.

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
