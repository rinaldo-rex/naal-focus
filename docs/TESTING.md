# Testing and verification

[Quick start](../README.md) · [Reference](REFERENCE.md) · [Architecture](ARCHITECTURE.md)

Automated tests establish scheduling, settings, rendering, and shared-control behavior. They do **not** establish that a visible overlay captures and restores focus correctly on every COSMIC installation. Use the manual checks below for that boundary.

## Automated and read-only checks

Run from the repository root:

```sh
just check          # Formatting, Clippy, applet-enabled unit tests, shell syntax
just check-native   # Three-client supervisor test in a private D-Bus session/runtime
just probe          # Read-only check of the current desktop, outputs, and guards
```

For core/helper-only testing without compiling libcosmic:

```sh
cargo test --locked
```

`just check` skips the native integration test by design. Run `just check-native` separately inside an active, unlocked COSMIC Wayland session with `dbus-run-session` available. Its script isolates the session bus, runtime locks, and settings file, but still connects to the real desktop compositor and system logind. It never enables recurrence or requests an overlay and does not contact the installed applet's control service.

`just probe` creates no surfaces and captures no focus. It reports monitor names, logical dimensions, scale factors, and session-guard status. Read the printed guard status as well as the exit code: a completed probe can report that reminders are blocked. Protocol availability is not proof of successful rendering or focus restoration.

### Coverage

| Area | Automated coverage | Not established by these tests |
|---|---|---|
| Settings and quotes | Defaults, numeric/schema limits, typing-limit bounds/backward defaults, multiline parsing and round trips, atomic-save round trips, corrupt-file preservation | Power-loss recovery, permissions failures, complete popup interactions |
| Reflection state | UTF-8 editing/selection, exact countdown holds/resumes, save-failure drafts, stale acknowledgments, fixed typing deadline, saved-text reopening/cancellation, no pause on click alone | Full keyboard/input-method integration and all live cancellation paths |
| Activity persistence | Private permissions, symlink/hardlink refusal, lock contention, incomplete/malformed history preservation, partial-write rollback, delayed worker acknowledgment, idempotent retries, atomic same-ID corrections, preserved timestamps/legacy records, stale-correction refusal | Real power loss, filesystem stalls, and device failure |
| Scheduling | Full intervals, pause/snooze/resume, quote order, no overlap, unsafe-session and time-gap resets | Real suspend/wake and lock/unlock transitions |
| Safety | Missing/stale observations block reminders; idle and lock reasons propagate | All live logind, COSMIC lock-marker, and input-seat transitions |
| Process lifetime | Monotonic boot clock, singleton exclusion/release, child kill-and-reap on drop | Stalled renderer watchdog and parent-death signaling under live overlays |
| Overlay input and rendering | CLI/stdin limits, parent/helper budget propagation, multiline layout, wrapping, premultiplied pixels, input hit-testing/scale/scroll, progress pixels, cached-background preservation across saved/editing transitions | Wayland surface lifecycle, visible readability, keyboard suppression, dismissal, focus restoration, output hotplug |
| Native applet helpers | Multiline editor content and Resume request selection | Dock/panel popup layout, unsaved-edit synchronization across visible windows |
| Shared supervisor | Three clients share settings/snooze, corrupt-file repair, lock-overlap retry, secondary shutdown, owner takeover, snooze retention | Cross-process crash scenarios, live helper cancellation during takeover |

## Latest recorded verification

Checked on **2026-10-04** against the activity-logging application source at `b8eea096`:

| Check | Result |
|---|---|
| `cargo test --locked` | 44 unit tests passed; native integration test skipped as intended. |
| `just check` | Formatting, warnings-denied Clippy, 46 applet-enabled unit tests, and shell syntax passed; native integration test skipped as intended. |
| `just check-native` | The isolated three-client supervisor regression passed. |
| `just probe` | Desktop layer-shell/shared-memory interfaces available; guards reported active, unlocked, and not idle. No surfaces created. |

Environment: Rust/Cargo 1.95.0; COSMIC Wayland; `cosmic-comp` package `0.1~1790118748~24.04~0fbd457`; `cosmic-panel` package `1.9.0~1790190460~24.04~b5cc19e`. The probe reported `eDP-1` at logical 1707×960, scale 2, and `HDMI-A-1` at logical 1920×1080, scale 1.

### Consented live reflection test

Using debug binaries, temporary `HOME` directories, and explicit one-shot commands (no installation or real activity-history writes), both outputs configured and acquired keyboard focus. The user confirmed immediate typing, a subtle textbox/progress bar, saving, and focus restoration.

The first interaction revealed slow typing updates and a deadline cutting short the post-save countdown. Both were corrected before the final test: immutable quote content is cached in two reusable buffers per output, and the fixed typing deadline applies while writing, not to the resumed breathing countdown. An offscreen debug renderer measurement across the two output sizes dropped from approximately 341 ms for a full repaint to 36 ms for an input-strip update; this is not an end-to-end latency guarantee.

In the final live test (`--delay-secs 15 --duration-secs 15 --typing-limit-secs 60`):

- Enter replaced the textbox with accepted text; clicking it reopened the editor, and correcting/resaving worked.
- A snapshot of the first submission and the final history showed **one record**, the same opaque ID and UTC timestamp, and corrected text—not a second entry.
- The helper dismissed both surfaces with reason `timeout` and exit status 0 after 41 seconds including the initial delay; it did not terminate through the typing-limit deadline.
- The user confirmed that typing was now responsive and the visible interaction worked as intended. A shortcut-specific Ctrl+A check was declined; no native shortcut acceptance is claimed.

**Still unverified live in this pass:** draft Escape/background cancellation, no-input dismissal on the final build, typing-limit expiry with an unfinished draft, actual save failures, live settings-popup editing, long-writing recurring reminders, lock/idle/suspend/hotplug transitions, and stopped-helper/parent-death scenarios. Automated coverage is not substituted for these checks.

## Manual acceptance checks

**These checks intentionally capture keyboard focus.** Use a disposable editor document, save other work first, and run only with explicit consent. Start with a read-only probe to identify outputs.

```sh
just build
./target/debug/naal-focus-overlay --check
# Temporary activity history; no writes to your real ~/.naal-focus/:
test_home=$(mktemp -d)
HOME="$test_home" ./target/debug/naal-focus-overlay --once --all --delay-secs 3
# Replace OUTPUT_NAME with a name printed by the probe:
HOME="$test_home" ./target/debug/naal-focus-overlay --once --output OUTPUT_NAME --delay-secs 3
# More time for deliberate typing/correction checks:
HOME="$test_home" ./target/debug/naal-focus-overlay --once --all --delay-secs 15 --duration-secs 15 --typing-limit-secs 60
```

| Check | Expected result |
|---|---|
| All-monitor presentation | Every connected monitor dims; one quote is readable across the tested screen sizes and scales. |
| No-input timeout | Without typing, no log is created and every overlay disappears after the normal duration. |
| Reflection input and progress | Typing works immediately and does not reach the application underneath; a fixed underline progress bar appears while the normal countdown is held. |
| Submission and correction | Enter leaves accepted text, resumes the remaining normal countdown, and writes one private record. Clicking reopens without pausing until a text change; resaving corrects that record without changing its original timestamp. |
| Escape and click-release | With a draft, each discards it and resumes the remaining countdown, restoring earlier accepted text for corrections. Without a draft, each dismisses all monitors. A background cancel must not click the application underneath; a field click must not cancel. |
| Deadline and persistence failure | An unfinished draft closes at the typing limit with no auto-save. Cancel/reopen cannot renew the allowance. A save error retains the draft without claiming success; watchdogs remain active. |
| Focus restoration | After each dismissal method, the intended application accepts typing again. Restoration is compositor-controlled. |
| Multiline quotes | Hard line breaks, blank paragraphs, literal bullets, long words, and long quotes remain understandable. Use a suitable duration for reading. |
| Panel and standalone controls | Both display shared saved settings/status; another client's save does not overwrite unsaved edits. Resume enables paused reminders or clears a snooze. |
| Recurring reminders and snooze | Opt in using controls. No reminder appears before a full interval; snoozing cancels visible/pending reminders, then waits the snooze plus a full interval. Disable recurrence afterward. |
| Lock, idle, and suspend | Through the supervised applet, unsafe sessions suppress/cancel reminders. Returning waits a full interval with no catch-up burst. Save work before lock/suspend. |
| Output hotplug and scaling | Adding/removing outputs during a visible reminder dismisses all surfaces. Repeat presentation tests at supported scales; fractional scaling is a documented limitation. |
| Supervisor/helper failure | In an isolated test setup, helper failure pauses recurrence; a stalled helper is terminated by its parent watchdog, and owner exit leaves no helper behind. |

A helper launched directly from a terminal has no independent parent supervisor. SIGSTOP freezes its own timeout and watchdog too; do not use that mode to test stalled-helper safety. Use an isolated supervised setup with a separate recovery terminal instead. Input-seat topology changes require an applet restart.

For each live test, record the application revision, date, COSMIC/toolchain versions, output names/scales, dismissal method, observed focus behavior, and pass/fail or untested status. Update this document when results change; do not infer acceptance from a successful probe.
