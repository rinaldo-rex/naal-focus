# Testing and verification

[Quick start](../README.md) · [Reference](REFERENCE.md) · [Architecture](ARCHITECTURE.md)

Automated tests establish scheduling, settings, rendering, and shared-control behavior. They do **not** establish that a visible overlay captures and restores focus correctly on every COSMIC installation. Use the manual checks below for that boundary.

## Automated and read-only checks

Run from the repository root:

```sh
just check          # Formatting, Clippy, applet-enabled unit tests, shell syntax
just check-native   # Three-client supervisor test in a private D-Bus session/runtime
just check-settings # Opens an isolated settings window; no reminders or overlays
just probe          # Read-only check of the current desktop, outputs, and guards
just preview-breathing # Offscreen native PPM frames/timings under /tmp; no surfaces
```

For core/helper-only testing without compiling libcosmic:

```sh
cargo test --locked
```

`just check` skips the native integration test by design. Run `just check-native` separately inside an active, unlocked COSMIC Wayland session with `dbus-run-session` available. Its script isolates HOME/history, XDG directories, the session bus, runtime locks, and settings file, but still connects to the real desktop compositor and system logind. It never enables recurrence or window tracking, requests an overlay, or contacts the installed applet's control service.

`just check-settings` is a separate opt-in native UI-process regression. It launches concurrent settings processes on a private bus/runtime with temporary preferences, checks that only one survives, verifies reopening reuses its PID, and terminates/relaunches that process while the separate supervisor stays available. The private bus does not auto-start desktop services. Recurrence remains paused and no overlay is requested. This does not establish visible layout, token-authorized focus, or header-bar close-button behavior.

`just probe` creates no surfaces and captures no focus. It reports monitor names, logical dimensions, scale factors, session-guard status, and focus-history protocol availability from the registry without requesting window metadata. Read the printed guard status as well as the exit code: a completed probe can report that reminders are blocked. Protocol availability is not proof of successful rendering or focus restoration.

### Coverage

| Area | Automated coverage | Not established by these tests |
|---|---|---|
| Settings and quotes | Defaults, numeric/schema limits, typing-limit bounds/backward defaults, multiline parsing and round trips, atomic-save round trips, corrupt-file preservation | Power-loss recovery, permissions failures, complete popup interactions |
| Breathing entrance | Seed determinism/variation, all four bounded geometries, fixed constellation links, family-subset selection, all pace/count/enable settings and CLI round-trips, strict invalid-input rejection, legacy defaults, shared preferences across supervisor takeover, dirty/post-save drafts, exact reflection/typing budgets, cue/amplitude boundaries, repeated transparent arrival frames, clipping/premultiplied pixels and repeatable redraw at desktop sizes/1×/2×, exact cached reveal endpoints | Live keyboard suppression and Escape, cross-monitor smoothness, actual compositor buffers/damage, installed settings controls, focus restoration, and safety transitions during the entrance |
| Reflection state | UTF-8 editing/selection, exact countdown holds/resumes, save-failure drafts, stale acknowledgments, fixed typing deadline, Enter-to-reopen at the UTF-8 text end, unchanged Enter/Esc closure without submission, saved-text reopening/cancellation, no pause on reopening alone | Full keyboard/input-method integration and all live cancellation paths |
| Reflection recall | Current-local-day filtering/future exclusion, stable chronological grouping of consecutive repeats, latest-age/count labels, rolling bounds/reversal, draft/caret/selection preservation, Enter-recall versus submission, fixed browsing allowance, nonblocking worker failure, keyboard-only rows, highlighted-only metadata, 70/40/20 fading, cached-quote preservation, desktop sizes and 2× scaling, fresh-record reuse without modifying the source | Live COSMIC key routing, animation smoothness, and simultaneous dial motion across monitors |
| Activity persistence | Private permissions, symlink/hardlink refusal, lock contention, incomplete/malformed history preservation, partial-write rollback, delayed worker acknowledgment, idempotent retries, atomic same-ID corrections, preserved timestamps/legacy records, stale-correction refusal, lenient per-line scan skipping malformed/blank/duplicate/oversized lines | Real power loss, filesystem stalls, and device failure |
| Activity dashboard | Tag parsing, civil-date round trips, Monday week bucketing, day-presence streaks, attention-share aggregation, escaping golden tests (`</script>`, quotes, `&`), empty/error/skipped-line states, writer permissions/link refusal/rewrites, browser command routing | Visual layout across other browsers and themes; the failure status line in a broken environment (the click path and page itself were live-accepted at the panel, below) |
| Overlay charts | Backward-compatible off defaults, both styles and all category combinations reaching the helper, settings-draft retention, local-day/hour counts, future exclusion, per-entry deduplication, same-ID corrections/snapshot upserts, legacy records, overflow conservation, day rollover, proportional treemap partitions, heatmap future/zero/intensity pixels, clipping and quote preservation at desktop sizes/scales | Live settings dropdown/toggles, cross-monitor chart refresh, and compositor damage/buffer-release behavior |
| Focus history | Off/backward-compatible defaults, all three styles and both sides through shared settings/helper CLI, title/app bounds, monotonic durations and fractional carry, title switches, unknown/own-window gaps, stale heartbeat, suspend/restart/clock jumps, day purge and crash-scratch cleanup, private atomic storage/link refusal, record cap, conserved Other totals, cached sidebar clipping alongside charts/input, scaled pointer coordinates, compact/portrait/ultrawide/HiDPI desktop outputs | Actual COSMIC activation/title delivery, live idle/lock/suspend/rollover behavior, installed controls, multi-monitor compositor buffers, abrupt process exit during a flush |
| Scheduling | Full intervals, pause/snooze/resume, quote order, no overlap, unsafe-session and time-gap resets | Real suspend/wake and lock/unlock transitions |
| Safety | Missing/stale observations block reminders; idle and lock reasons propagate | All live logind, COSMIC lock-marker, and input-seat transitions |
| Process lifetime | Monotonic boot clock, singleton exclusion/release, child kill-and-reap on drop | Stalled renderer watchdog and parent-death signaling under live overlays |
| Overlay input and rendering | CLI/stdin limits, parent/helper budget propagation, multiline layout, wrapping, premultiplied pixels, input hit-testing/scale/scroll, background press/release no-ops across draft/saved/correction states, field reopening, progress pixels, cached-background preservation across saved/editing transitions | Wayland surface lifecycle, visible readability, keyboard suppression, dismissal, focus restoration, output hotplug |
| Native applet helpers | Multiline editor content, Snooze/Resume selection, desktop launch environment, settings validation, clean/dirty synchronization, failed-save and post-save draft retention | Dock/panel popup layout, unsaved-edit synchronization across visible windows |
| Settings-window process | Concurrent startup exclusion, existing-process activation, window-lock release/relaunch, supervisor survival after settings-process termination | Unsaved-edit synchronization across visible windows; the rest (visible layout, click-outside persistence, header-bar close, real field/button interactions) was live-accepted at the panel on 2026-10-06 |
| Shared supervisor | Three clients share settings/snooze, corrupt-file repair, lock-overlap retry, secondary shutdown, owner takeover, snooze retention | Cross-process crash scenarios, live helper cancellation during takeover |

## Latest recorded verification

### Seeded breathing entrance (2026-10-28)

- User approved all four pattern families, enabled by default, one 4s-in/6s-out breath before reflection, early typing ignored, and settings for on/off, reduced motion, pace, one/two breaths, and family selection. The entrance takes 14 seconds by default and at most 24 seconds. Both helper and supervisor watchdog budgets include it; recurring-reminder and tracking opt-ins remain unchanged.
- `RUST_TEST_THREADS=1 just check` passed formatting, warnings-denied Clippy, **144 applet-enabled unit tests**, and shell syntax. `RUST_TEST_THREADS=1 cargo test --locked` passed **131 core/helper tests**. `just check-native` passed with nondefault breathing pace/count/families/reduced motion propagating through three clients, persistence, and supervisor takeover. No overlay or collection was requested.
- A subsequent targeted native raster test and warnings-denied Clippy pass also verified repeated arrival frames with the same quantized alpha do not introduce an opaque central patch. No claim of live compositor acceptance is made.
- `just probe` reported ready guards, DP-2 and DP-3 at 1920×1080 logical / scale 1, and eDP-1 at 1707×960 logical / scale 2. No surfaces or window metadata were requested.
- Offscreen native snapshots of every family (seed 42, peak breath) were visually inspected. `cargo run --locked --release --example breathing-preview -- /tmp/naal-breathing-native` exports PPMs and measures geometry/reveal work without connecting to Wayland. Representative geometry averages were **0.4–0.8 ms** at 1920×1080/1× and **1.5–2.7 ms** at 3414×1920/2×; full-surface reveal averaged **1.2 ms** and **3.8–3.9 ms** respectively. These local renderer timings are not end-to-end animation/focus guarantees.
- At the pre-install checkpoint, no real applet reload, settings window, overlay, recurrence/tracking opt-in, or real preference/history changes had been performed. Live acceptance remains required for smooth multi-output fade/reveal, keyboard/Enter/pointer suppression during the entire entrance, immediate Escape, full post-entrance reflection time, buffer restoration without ghost patterns, disabled/reduced-motion modes, settings interactions, focus restoration, and guard/hotplug/watchdog behavior.
- With subsequent explicit user approval, the release was installed under `~/.local` and the COSMIC panel reloaded. Installed applet/helper hashes match the release build; all three running applet executables match the updated applet. The settings-file checksum and every existing shared preference remained unchanged. Shared status reports breathing enabled with all four families, one balanced breath, reduced motion off, ready guards, and no control/tracking errors. No test overlay or settings window was launched. Pre-install binaries and settings were retained in a private temporary backup directory for this rollout; existing enabled recurrence was preserved, not newly enabled.

### 12/24-hour focus clocks (2026-10-28)

- Focus-sidebar clocks now default to 12-hour AM/PM, including existing settings with no clock-format field. **Settings → Focus history → 24-hour time** selects 24-hour header, hourly ticks, and end time through the existing draft/save path. Durations and reflection-dashboard times are unchanged.
- `RUST_TEST_THREADS=1 just check` passed formatting, warnings-denied Clippy, **134 applet-enabled unit tests**, and shell syntax. `RUST_TEST_THREADS=1 cargo test --locked` passed **122 core/helper tests**. `just check-native` passed shared settings/takeover with a nondefault 24-hour preference, without enabling tracking.
- Tests cover legacy nested settings, both format round-trips and helper flags (including tracking disabled), dirty/post-save settings drafts, midnight/noon/evening labels, and measured label/bar separation at 1×/2×. Sidebar clipping/premultiplied-pixel tests exercise both formats, all three graphs, both sides, and compact, standard, portrait, ultrawide, and HiDPI desktop outputs. Fictional native full-timeline and compact-combined AM/PM frames spanning noon were visually inspected; these were not live overlays.

- With explicit user approval, the release was installed under `~/.local` and COSMIC's panel reloaded. Both installed binaries matched the release build; all three applet processes ran the updated binary. The settings-file checksum and existing reminder/tracking/graph/side preferences were unchanged. Shared status confirmed the 12-hour upgrade default (`use_24_hour_time: false`) with no control/tracking errors. No test overlay or settings window was launched; live toggle clicks remain unverified.

### Today-only foreground-window history (2026-10-28)

- User selected all three mockups as Settings options, both sidebar positions, **Timeline + totals / Right** defaults, application IDs plus window titles, today-only retention, and tracking independent of paused/snoozed reminders. Tracking remains off by default, including upgrades.
- `RUST_TEST_THREADS=1 just check`: formatting, warnings-denied Clippy, **132 applet-enabled unit tests**, and shell syntax passed. `RUST_TEST_THREADS=1 cargo test --locked`: **120 core/helper unit tests** passed.
- One unrestricted parallel run intermittently failed the unchanged activity-log correction/reuse tests with `Activity history is busy; try again.` All tests passed when serialized; the parallel lock-contention cause was not diagnosed or changed in this feature.
- `just check-native` passed the isolated three-client supervisor regression, including focus-preference propagation/takeover, tracker singleton exclusion, and eventual tracker-lock release. The script now isolates HOME/history and XDG directories as well as D-Bus/runtime, so default-off retention cannot touch real history. It did not enable collection or recurrence.
- Accounting/storage tests cover switches and title changes, repeated intervals, unknown/own-window gaps, stale supervisor heartbeat, suspend/stall/restart gaps, backwards/forwards clock jumps, small clock slews, sub-millisecond event bursts without rounding loss, session bounds and next-day recovery, conserved overflow totals, private permissions, busy/invalid/future history, link/nonregular refusal, stale-day deletion, crash scratch removal, and atomic replacement. Cleanup errors remain visible even while collection is off.
- Native raster tests cover 1000×650, 1920×1080, ultrawide 2560×1080, portrait 1080×1920 (scale 1), and 3414×1920 physical / 1707×960 logical (scale 2), with all three graph styles and both sides. Additional tests preserve quote/sidebar pixels while mention heatmaps/treemaps and reflection editing repaint, and retain scaled field hit-testing/background-click behavior. Fictional native sample frames for the timeline, combined view, and compact left sidebar were visually inspected. These are raster frames, not live Wayland acceptance.
- `just probe` reported ready safety guards and both required focus protocols, without requesting window metadata or creating surfaces. Outputs were DP-2 and DP-3 (1920×1080 logical, scale 1) and eDP-1 (1707×960 logical, scale 2).

**Still unverified live:** enabled tracker activation/title events, installed graph/side controls, real private/incognito-title handling, elapsed-time accuracy during live switching, cross-output buffers, and actual idle/lock/suspend/midnight transitions. At that initial feature-verification checkpoint, no installation, focus-capturing overlay, settings window, real tracking, or changes to real history/preferences were performed. Later installation/reload checks are recorded above.

### Overlay charts (issue #9)

Checked against feature revision `6e89d70d`:

- User selected the hourly heatmap (mockup B) and treemap (mockup E), one shared style with separate Projects / Feelings / Tags switches, and all categories off initially.
- `RUST_TEST_THREADS=1 just check`: formatting, warnings-denied Clippy, **106 applet-enabled unit tests**, and shell syntax passed. `RUST_TEST_THREADS=1 cargo test --locked`: **95 core/helper unit tests** passed.
- Aggregation tests cover local versus UTC midnight, today/future filtering, case/repeated-value deduplication, per-hour bins, legacy ID-less moments, successful-write upserts, same-ID correction replacement/removal, overflow mention conservation, and day/hour rollover. CLI propagation checks both styles with all eight category combinations; settings tests retain chart drafts through shared updates and post-save edits.
- Offscreen native rendering tests cover 800×600, 1000×650, 1920×1080, ultrawide 2560×1080, portrait 1080×1920 (scale 1), and 3414×1920 physical / 1707×960 logical (scale 2). They check band separation, clipping, long labels/overflow, loading/empty/error states, premultiplied pixels, future versus zero/intensity heatmap cells, and treemap area proportions. Native sample frames for both styles at 1000×650 and treemap at 1920×1080 were visually inspected, as was a scale-2 heatmap. These are native raster frames, not browser approximations or live Wayland acceptance.
- `just check-native` could not run: this agent session has no `WAYLAND_DISPLAY`. The existing isolated supervisor regression now includes chart preferences in its shared-save/takeover assertions, but its live execution remains unverified here.

**Still unverified live:** native settings dropdown/toggles, synchronized chart updates after save/correction, cached chart refresh with real compositor buffers, and tiny-tile readability on actual monitors. No overlay/settings window was launched, installation performed, or real history/preferences changed.

### Keyboard correction shortcut (issue #10)

- `RUST_TEST_THREADS=1 just check`: formatting, warnings-denied Clippy, **95 applet-enabled unit tests**, and shell syntax passed.
- Enter policy tests cover saving, reopening with an unselected caret at the end of Unicode text, changing and resaving, closing unchanged text with Enter/Esc without submission, expiry refusal, and preservation of both countdowns. The recall test now exercises the same Enter policy: the first press copies; the next submits.
- Existing persistence tests still establish same-ID/same-timestamp corrections, and pointer tests retain click-to-edit behavior. The keyboard route handles both Return and keypad Enter before the hidden-field guard; the existing held-repeat and in-flight-save guards remain in place.

**Still unverified live:** Return/keypad Enter routing, held-key suppression after save acknowledgment, and synchronized reopening across monitors. No overlay was launched, installation performed, or real history/settings changed.

### Inline metadata and autocomplete (issue #6)

Checked on **2026-10-15**:

- `RUST_TEST_THREADS=1 just check`: formatting, warnings-denied Clippy, **92 applet-enabled unit tests**, and shell syntax passed. Parallel runs exposed intermittent `Activity history is busy` failures in existing persistence tests; each passed alone. The suite already documents brief CLOEXEC descriptor inheritance across parallel process launches in its singleton test, so the final run used one test thread without changing production locking.
- Parser/storage coverage includes independent case-insensitive tag/project/feeling values, legacy records, Unicode combining marks and lowercase expansion, matching metadata validation, atomic corrections, and the maximum reflection length. Vocabulary tests include earlier days while excluding future records and retaining only today's full recall entries.
- Editor coverage includes caret-local completion for all three kinds, whole-token replacement inside a sentence, selection exclusion, suggestion dismissal without cancellation, character limits, and preservation of the original typing deadline. Enter never automatically inserts a suggestion.
- Offscreen native rendering checked pills, suggestions, overflow, clipping, cached quote preservation, and premultiplied pixels at 1000×650 and 1920×1080 (scale 1), and 3414×1920 physical pixels (1707×960 logical, scale 2). Native sample frames at 1000×650 and scale 2 were visually inspected. No overlay was opened.
- A generated dashboard fixture was checked in the collaborative desktop browser at 1280×800: project/feeling overview counts, all three metadata pill kinds in day and month details, and no horizontal page overflow. Long metadata values (650 characters per kind) also stayed within the day timeline. The browser console was clean.
- Read-only `just probe` reported ready guards and DP-1 / HDMI-A-1 (1920×1080, scale 1) plus eDP-1 (1707×960 logical, scale 2). No surfaces were created or keyboard focus captured.

**Still unverified live:** suggestion key routing, first-Escape behavior, pill updates while typing, and synchronized suggestion selection across monitors. No installation or changes to real history/settings were made.

### Reflection recall dial (issue #8)

Checked on **2026-10-12** against feature revision `55b7c464`:

- The interactive temporary HTML preview was reviewed and approved before native implementation: three-row rolling window through today's history, consecutive repeat groups, arrows-only browsing, Enter copies then Enter saves, original draft retained, and 70% / 40% / 20% bottom/middle/top opacity. Relative age and multiplier appear only on the highlighted row. Browsing was explicitly approved to share the fixed typing allowance.
- `cargo test --locked`: **72 core/helper unit tests passed**.
- `just check`: formatting, warnings-denied Clippy, **82 applet-enabled unit tests**, and shell syntax passed.
- `just check-native`: isolated three-client supervisor regression passed; no overlay requested and installed controls untouched.
- Offscreen native rendering checked the dial, highlighted-only metadata, reserved quote/input separation, UTF-8/long rows, and premultiplied pixels at 1000×650 and 1920×1080 (scale 1), and 3414×1920 physical pixels (1707×960 logical, scale 2). Native sample frames were also visually inspected; this is not live Wayland acceptance.
- Read-only `just probe` reported active/unlocked/not-idle guards and outputs DP-1 (1920×1080, scale 1), eDP-1 (1707×960 logical, scale 2), and HDMI-A-1 (1920×1080, scale 1). No surfaces were created and no focus captured.
- History tests verify filtering without writes, malformed/oversized-line handling, a stalled loader's nonblocking polling, and reuse creating a fresh ID/timestamp while later correction leaves the historical source intact. The shared scanner now drains oversized lines in bounded chunks without consuming the next good record.

**Still unverified live:** native recall key routing, rolling animation smoothness, synchronized dial motion on multiple monitors, and focus restoration with this feature. No live overlay was launched during this work; installation leaves applet reload to the user.

### Settings-window separation

Checked on **2026-10-04** at `552ef9a7` (feature code: `0749de2a`):

- `just check`: formatting, warnings-denied Clippy, 52 applet-enabled unit tests, and shell syntax passed.
- `just check-native`: isolated three-client supervisor regression passed.
- `just check-settings`: isolated concurrent-launch / reopen / process-termination / relaunch regression passed.
- `cargo test --locked`: 44 core/helper unit tests passed.

No installation or changes to real reminder preferences were made. The first native settings test passed its assertions but auto-started desktop portals whose temporary mounts interfered with cleanup; the harness was changed to disable service auto-start, and the complete command then passed cleanly.

**Still awaiting manual acceptance was resolved on 2026-10-06** — see the live acceptance below; note that process activation alone still does not prove compositor focus or raise behavior, which is why this pass was manual.

### Settings-window live acceptance

**Accepted at the real panel on 2026-10-06** (installed build `552ef9a7`, whose #3 feature code is identical to current main):

- The popup shows only status, the reminders toggle, Show now, Snooze/Resume, and Settings….
- Settings… opens the grouped window from the panel; with the window minimized, reopening raised the same window instead of creating a duplicate.
- Clicking outside keeps the window open; closing via the header bar left the applet processes and scheduling running (verified from the process list and settings file during the session).
- The window resizes with its groups remaining readable.

The live review surfaced two feedback lessons, implemented the same day and re-validated on the debug build at the polish revision:

- Invalid numeric input renders its range message directly under the affected field in the destructive theme color, while typing.
- A successful save shows a prominent "✓ Settings saved" banner at the top of the window, auto-clearing after some seconds.
- Enter in a numeric field saves; leaving the settings window auto-saves a wholly valid draft (observed live: a changed interval reached the settings file without pressing Save). An invalid draft, e.g. an interval of 5, saves nothing, keeps its values, and keeps its inline errors.

### Activity dashboard

Checked on **2026-10-08** against the dashboard application source at `e39b78b3` with applet wiring `dc52fa38`:

- `cargo test --locked`: 57 core/helper unit tests passed, including the new dashboard suite (escaping, tags, civil dates, bucketing, streaks, writer guards).
- `just check` (applet-enabled): formatting, warnings-denied Clippy, and unit tests passed.
- Generated pages with a rich fixture, an empty history, and a simulated unreadable history were opened in a real desktop browser from `file://`:
  - Overview: 53 week columns × 7 day squares, future days blank, intensity by count, attention share of tagged weeks only, tag counts, streak circles with current/longest/fraction, gentle phrasing.
  - Day: timeline with 12-hour times (`11:04 AM`-style), relative toggle (`43 mins ago`-style), quiet gap markers including the closing one, ‹ › day walking, 24-hour dial with band and pins at true minutes.
  - Month: calendar with weekend shading, today outline, count badges, tag dots, multi-tag OR filter dimming non-matching days to unclickable, day detail below.
  - Hash routing (`#day`, `#month`) and year-map square → day deep navigation worked; the console stayed clean.
  - **Download HTML** saved a self-contained copy that rendered offline from the saved file.
  - The empty fixture showed the friendly "Nothing recorded yet" hero; the unreadable fixture showed the restrained "History could not be read" panel with the reason rendered as text.

**Live acceptance at the real panel on 2026-10-08** (installed build, issue #2): the **Dashboard…** click path worked end to end — regenerated from the live history, wrote `~/.naal-focus/dashboard.html` (0600), opened the default browser, and closed the popup; the page's views, filters, and interactions were exercised on the real data. One annotated feedback round was raised and resolved the same day (`8340aae0`): the year map needed a horizontal scrollbar at full desktop width (center column widened 1100 → 1280px) and the weekday/month rails misaligned (now GitHub-style, re-verified programmatically at the annotated 1707×764 viewport with no overflowing element and a clean console). Still untested live: the failure status line in a home-less or unreadable environment (covered by unit tests).

### Activity-logging verification

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

**Still unverified live after this pass:** actual save failures, live settings-popup editing, long-writing recurring reminders, lock/idle/suspend/hotplug transitions, and parent-death signaling. Automated coverage is not substituted for these checks.

### Live cancellation, deadline, and watchdog regression

Checked on **2026-10-06**. One-shot runs used the debug build at `5df4b3ea` (activity-logging feature code `b8eea096`) on COSMIC Wayland with outputs `DP-1` (1920×1080, scale 1), `eDP-1` (1707×960 logical, scale 2), and `HDMI-A-1` (1920×1080, scale 1). Temporary `HOME` directories kept real activity history untouched. Real reminders were paused during the session and re-enabled afterward.

| Check | Result |
|---|---|
| Escape while writing | Draft discarded, remaining countdown resumed to a `timeout` dismissal; no record. |
| Background click-release while writing (historical, before issue #7) | Previously discarded the draft and resumed the countdown; superseded by issue #7, which makes background clicks no-ops. The user confirmed the click did not reach the application underneath. |
| No-input dismissal (final build) | Untouched overlay dismissed `timeout` after the normal duration; no record, no input-strip redraw. |
| Escape with no draft | Immediate dismissal (`escape`); no record. |
| Cancel/reopen draft handling | A second draft after a cancel cycle was still discarded — not dismissed — on Escape; the countdown resumed to a `timeout` dismissal. |
| Typing-limit expiry | With a 30s limit and a cancel/reopen attempt partway through, dismissal reason was `typing-limit` at first input + 30s; cancel/reopen did not renew the deadline and the unfinished draft was not saved. |
| Shared-monitor input | The user confirmed typed text and the underline progress bar appeared simultaneously on all three outputs. |
| Stalled-helper watchdog | A supervised helper (spawned by the installed applet via Show now; real settings: duration 10s, typing limit 120s) was frozen with SIGSTOP ~0.03s after detection. It remained in state `T` and was killed at the designed 138s helper-lifetime budget (`duration + typing limit + 8`). A stopped process cannot exit on its own, so the kill came from the parent watchdog. No leftover helper; the applet processes stayed healthy. This exercised the installed release build `552ef9a7`; the watchdog code (`engine.rs`, `config.rs`, `process.rs`) is unchanged through `5df4b3ea`. |

Observation: a scheduled real reminder collided with one one-shot run; the singleton guard refused the second overlay (`Another Naal Focus overlay is already running`, exit 1) instead of stacking another focus-capturing surface.

**Still unverified live in this pass:** actual save failures, live settings-popup editing, long-writing recurring reminders, lock/idle/suspend/hotplug transitions, and parent-death signaling. Long-writing recurrence and guard cancellations remain established by automated tests only.

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
| All-monitor presentation | Every connected monitor fades into the same seeded geometry on the same breath clock. Quote/history/charts/sidebar appear only after breathing; verify supported desktop sizes/scales and no pattern ghosts on either reused buffer. |
| Breathing controls and input gate | Settings → Breathing entrance saves on/off, each pace, one/two breaths, family subsets, and reduced motion; no-family selection reports an error. During entrance, type, hold a key, press Enter/arrows, and click the future field: no draft/history/save/typing timer starts. Escape dismisses all outputs immediately. With reduced motion the shape stays still while cues change. Disabled mode restores immediate reflection. Verify installed window layout and shared draft synchronization. |
| No-input timeout | Without typing, no log is created. Every overlay disappears after the full entrance plus normal reflection duration (default 14 + 5 seconds), or just the normal duration with breathing off. |
| Metadata and autocomplete | Type `Review @naal #rust ??focused`: pills update immediately, including partial words. Delete/edit a mention and verify its pill updates. Save, reopen, correct, and confirm one ID/timestamp with recomputed metadata. With earlier-day vocabulary, type each marker; arrows select matching values and Tab inserts without extending the deadline. Enter saves as written. Esc closes suggestions first, then restores history navigation/cancellation. Check Unicode, long/overflowing values, caret-in-middle replacement, and identical suggestions on every output. |
| Reflection recall dial | Today's previous groups appear in a three-row fading window (70/40/20); only the highlighted bottom row shows latest age/count. ↑ selects newest then rolls older groups down; ↓ reverses without wrapping or crossing yesterday. Enter only copies into the input; another Enter saves a new timestamp/ID. Type a draft and browse: ↓ past newest or Esc restores its text/caret/selection. Browsing holds only the existing fixed typing budget. History/background clicks do nothing. Verify identical selection and rolling on all monitors, empty history, long rows, and local-day boundaries. |
| Overlay charts | In Settings → Overlay charts, verify charts start off, choose Heatmap or Treemap, enable each category and combinations, then Save. Reopen settings and verify persistence/shared synchronization and dirty-draft retention. Use known saved mentions: heatmap hourly cells and treemap daily totals match, repeated metadata counts once per record, earlier/future entries are excluded, overflow becomes Other. Submit then correct: charts refresh identically on every output without a second moment; drafts/recall/failed saves leave counts alone. Check loading/empty/unavailable states, day/hour boundaries, long/tiny labels, desktop sizes/scales, and separation from quote/history/input. Neither charts nor their background handle clicks or alter countdowns. |
| Reflection input and progress | After the entrance finishes, typing works immediately and does not reach the application underneath; a fixed underline progress bar appears while the normal countdown is held. |
| Submission and correction | Enter leaves accepted text, resumes the remaining normal countdown, and writes one private record. Enter again (also keypad Enter) reopens with an unselected caret at the end; clicking still positions the caret. Reopening alone does not pause. Edit and Enter: the same record/ID/timestamp is corrected. Reopen without changes: Enter/Esc closes without another write. Hold Enter through save acknowledgment: repeats must not reopen; presses during an in-flight save must not duplicate it. Verify synchronized reopening on every monitor. |
| Escape | With a draft, discards it and resumes the remaining countdown, restoring earlier accepted text for corrections. Without a draft, dismisses all monitors. |
| Background click-release | Must not cancel a draft/correction, resume its held countdown, or dismiss any monitor. Test untouched, writing, saved, and reopened states, plus a field-to-background drag. Clicks must not reach the application underneath. Field clicks must still position the caret or reopen accepted text. Issue #7 behavior is unit-tested; live acceptance remains untested. |
| Deadline and persistence failure | An unfinished draft closes at the typing limit with no auto-save. Cancel/reopen cannot renew the allowance. A save error retains the draft without claiming success; watchdogs remain active. |
| Focus restoration | After each dismissal method, the intended application accepts typing again. Restoration is compositor-controlled. |
| Multiline quotes | Hard line breaks, blank paragraphs, literal bullets, long words, and long quotes remain understandable. Use a suitable duration for reading. |
| Compact panel controls | Icon opens only status, reminders on/off, Show now, Snooze (or Resume while snoozed), Dashboard…, and Settings…. No preferences/editor; safe controls remain usable in a small popup. |
| Settings window | Settings… opens a grouped, resizable normal window. Clicking elsewhere leaves it open; reopening raises the same window, including when minimized. Closing via its header bar leaves panel reminders running. Saved values/status synchronize; another client's save or a failed save does not overwrite a draft. Typing after Save survives its later acknowledgment. |
| Focus history | With explicit collection consent and isolated history, enable tracking, visit two apps and two titles within one app, and verify all three graphs on each monitor. Compare both sides, large/small desktop outputs and scaling; type/save/correct while charts coexist. Pause/snooze reminders: recording continues; turn tracking off: recording stops. Idle/lock/sleep and supervised reminders must not add time. Restart after a snapshot flush: today's data returns without a gap being credited. Verify next-day cleanup, missing-protocol/error states, and that private titles never appear in arguments/status/logs. No duration should be interpreted as proof of attention. |
| Dashboard… | From the popup, it rebuilds the page fresh, writes `~/.naal-focus/dashboard.html` privately (0600), opens the default browser, and closes the popup. Views, tabs, filters, and Download HTML behave as documented in the reference; reopening regenerates. With history unreadable, a restrained error page opens and the popup reports the failure without crashing. |
| Recurring reminders and snooze | Opt in using controls. No reminder appears before a full interval; snoozing cancels visible/pending reminders, then waits the snooze plus a full interval. Disable recurrence afterward. |
| Lock, idle, and suspend | Through the supervised applet, unsafe sessions suppress/cancel reminders. Returning waits a full interval with no catch-up burst. Save work before lock/suspend. |
| Output hotplug and scaling | Adding/removing outputs during a visible reminder dismisses all surfaces. Repeat presentation tests at supported scales; fractional scaling is a documented limitation. |
| Supervisor/helper failure | In an isolated test setup, helper failure pauses recurrence; a stalled helper is terminated by its parent watchdog, and owner exit leaves no helper behind. |

A helper launched directly from a terminal has no independent parent supervisor. SIGSTOP freezes its own timeout and watchdog too; do not use that mode to test stalled-helper safety. Use an isolated supervised setup with a separate recovery terminal instead. Input-seat topology changes require an applet restart.

For each live test, record the application revision, date, COSMIC/toolchain versions, output names/scales, dismissal method, observed focus behavior, and pass/fail or untested status. Update this document when results change; do not infer acceptance from a successful probe.
