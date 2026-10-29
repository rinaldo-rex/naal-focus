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

Click the panel/dock icon for a compact popup with status, the recurring-reminders toggle, **Show now**, **Snooze 15 min** (or **Resume** while snoozed), **Dashboard…**, and **Settings…**. Detailed preferences and quote editing live only in the dedicated settings window, grouped into **Timing**, **Appearance**, **Breathing entrance**, **Overlay charts**, **Focus history**, and **Reflection**. The window stays open when clicking elsewhere. Reopening settings activates the existing window rather than creating another; `naal-focus --settings` uses the same entry point.

Settings live at `$XDG_CONFIG_HOME/naal-focus/settings.json`, falling back to `~/.config/naal-focus/settings.json`. Saves are atomic. Invalid, oversized, or unsupported-version files remain untouched until explicit **Save settings** replaces them.

| Setting | Default | Range |
|---|---:|---|
| Recurring reminders | Paused | Enabled/paused |
| Interval | 300 seconds | 15–86400; greater than duration |
| Reflection duration | 5 seconds | 1–30 seconds, after the entrance; also applies when reflections are off |
| Enable reflections | On | Off hides writing, past reflections, metadata/suggestions, hints, and reflection charts |
| Breathing entrance | On | On/off; separate from recurring reminders |
| Breath pace | 4s in / 6s out | 3s/4s, 4s/6s, or 5s/5s |
| Breaths | One | One or two |
| Breathing patterns | All four | Any nonempty subset of lattice, polygons, constellation, weave |
| Reduced motion | Off | Static geometry with timed breath cues and fades |
| Typing time limit | 120 seconds | 30–600 seconds; separate from duration |
| Text size | 36 logical pixels | 22–72 |
| Idle threshold | 120 seconds | 30–3600 |
| Chart style | Heatmap (hidden until a category is enabled) | Heatmap / Treemap; shared across categories |
| Chart categories | All off | Independent Projects / Feelings / Tags switches |
| Track window time | Off | Independent of reminders and snooze |
| Focus graph | Timeline + totals | Time spent / Day timeline / Timeline + totals |
| Focus sidebar | Right | Left / Right |
| 24-hour time | Off (12-hour AM/PM) | Applies to focus-sidebar clock and timeline labels |
| Quotes | Four reminders | 1–100; ≤500 characters each, including line breaks |

- **Save settings** applies edited fields together, cancels pending/visible reminders, and restarts the interval. The settings window follows shared saved values without overwriting unsaved edits. A failed save retains its draft.
- **Recurring reminders** persists immediately.
- **Snooze 15 min** is temporary. A full interval follows the snooze; it does not survive closing every instance.
- **Resume** replaces Snooze while snoozed. It clears the snooze, or enables reminders if they were paused. Otherwise use the recurring-reminders toggle to unpause.
- **Show now** waits two seconds before displaying a one-shot reminder. It works while recurrence is paused, but cannot bypass safety guards.

Every monitor popup and `naal-focus --settings` shares one timer through session D-Bus. Closing a secondary instance leaves reminders running. If the owner closes, another instance takes over with the remaining snooze and a fresh interval. Closing all instances stops reminders and the helper.

## Breathing entrance

The native overlay chooses one allowed family and a fresh random seed for each reminder. Geometry is generated once; only its breath amplitude changes afterward. Connections never pop in/out and exhaling retraces inhaling. Shape, color, and monotonic timeline are shared across outputs, with layout adapted to each desktop size/scale. Consecutive reminders may choose the same family with different geometry.

The entrance fades in for two seconds, guides one or two complete breaths, then fades out the geometry and reveals the normal overlay over two seconds. Default timing is **14 seconds**, or **24 seconds** with two 4s/6s breaths; the shorter pace takes **11 or 18 seconds**. Quote, reflection/history, charts, and focus sidebar stay hidden during breathing. The full normal reflection duration and fixed typing allowance are preserved. All typing, Enter, and field clicks are ignored until the reveal finishes; **Escape dismisses every output immediately**. Safety cancellation and both watchdogs remain active throughout.

**Settings… → Breathing entrance** offers on/off, reduced motion, pace, breath count, and four family switches. Selecting no families is a validation error, not a silent fallback. Reduced motion freezes the shape but retains the chosen timing, breath cues, and gentle fades. Switching the entrance off restores immediate reflection and the old lifetime budget. Existing version-1 settings default to breathing on, all four families, one balanced breath, and reduced motion off; recurrence remains unchanged.

The optional settings object is:

```json
"breathing": {
  "enabled": true,
  "reduced_motion": false,
  "pace": "balanced",
  "cycles": 1,
  "families": ["lattice", "polygons", "constellation", "weave"]
}
```

Other pace values are `short` (3s/4s) and `even` (5s/5s). The helper accepts `--breathing on|off`, `--breath-pace short|balanced|even`, `--breath-cycles 1|2`, `--breath-patterns lattice,polygons,constellation,weave`, and `--reduced-motion`. These are validated before creating surfaces. Manual helper runs still **capture keyboard focus** and require the consented testing procedure.

## Activity reflections and history

**Settings… → Reflection → Enable reflections** is on by default, including when loading existing version-1 settings. Turning it off hides the input, past reflections, metadata pills, suggestions, typing underline/hints, and all reflection charts across monitors. Breathing, the quote, and the separately enabled focus-history sidebar remain. The quote uses the freed height; the normal post-entrance duration still applies. Typing, Enter, arrows, Tab, and field clicks do nothing; **Esc** dismisses immediately. No reflection-history worker is started and no reflection is submitted. Stored history, quotes, chart choices, and the dashboard are unchanged; re-enabling restores the saved chart choices. Apply through the normal settings draft/save path. The helper accepts `--reflections on|off`; the supervisor forwards the switch explicitly and both watchdogs omit the unused typing allowance when off.

After the breathing entrance, the overlay has one shared reflection across all covered monitors. Its single-line textbox is ready for input without clicking; the quote remains prominent. No typing means no logging and no change to the normal dismissal countdown.

- Three rows above the textbox form a rolling dial of previous reflections from the current local day, loaded fresh for each reminder. Consecutive identical texts are grouped; separate runs of the same task remain separate. Newest is nearest the input, with bottom/middle/top text opacity **70% / 40% / 20%**. Only the highlighted row shows the latest occurrence's relative age and repeat count (for example, `5 min ago · ×3`). Long rows are visually shortened, never truncated in storage or recall.
- **↑** selects the newest group, then rolls older groups down into the highlighted bottom position; newer groups roll below it and disappear. **↓** reverses the dial. It stops at the first reflection of the local day, never wraps, and shows fewer than three rows at that boundary. History rows are keyboard-only; clicks do not recall them.
- Browsing preserves the original draft, caret, and selection. **↓ past the newest** or **Esc** exits browsing and restores that editor. Typing or clicking the field also returns to the original draft. **Enter while browsing** only copies the selected text into the editable input; a second Enter saves it as a new moment with a fresh identity and timestamp. Earlier records are untouched.
- First text input **or history browsing** pauses the normal countdown, retaining its exact remaining duration. The underline below the field shows the fixed typing allowance remaining. Exiting browsing resumes the countdown only if the original editor was not already writing. Displaying history without interaction holds nothing; missing history leaves the normal input usable, and a read failure shows a quiet unavailable hint.
- **Enter** saves nonblank text (up to 2000 characters) through a background worker. After acknowledgment, the textbox disappears into a quiet rendering of the accepted text and the remaining normal countdown resumes—not a fresh duration.
- Press **Enter again** to reopen the accepted text with the cursor at the end, or click it to position the caret. Merely reopening, moving the caret, or selecting text does not pause the countdown. Typing, replacing, or deleting text does. Enter safely corrects the same record, preserving its original timestamp; it does not add another log. Enter without changes closes the editor without saving; held Enter repeats are ignored.
- **Esc** cancels a writing draft or correction and resumes the remaining countdown. A correction restores the last accepted text. Closing a reopened but unchanged editor leaves the countdown untouched. Otherwise Esc dismisses all monitors. Background clicks never cancel or dismiss; field clicks still position the caret or reopen accepted text.
- The typing allowance starts at the first text change or history-browse action and never restarts during that reminder, even across browse/save/cancel/reopen cycles. While writing or browsing, reaching the limit dismisses all outputs and discards unsaved text. After saving or canceling, the remaining normal countdown may finish even if the typing allowance expires; further changes cannot renew it.
- Empty submissions are rejected. Failed saves retain the draft and display an error while the safety deadline stays active. Clearing all text is not a save or cancellation.
- Cancellation is not Undo for a submission already initiated with Enter: an in-flight explicit save can complete. Unsaved drafts are never stored automatically.

### Inline tags, projects, and feelings

Type `#tag`, `@project`, or `??feeling` anywhere in a reflection, for example `Reviewing @naal #rust ??focused`. Live pills above the field reflect the current text, including a word still being typed. Edit or remove the inline mention to update its pill. Saved reflections also show these pills. Pills stay in one row; long values shorten visually and excess pills become `+N` without losing data.

Values use Unicode letters, numbers, and combining marks, plus `_` and `-`; trailing `_`/`-` is ignored. Use `@deep_work` for a multiword project. Values are lowercased and deduplicated separately for each kind. Projects start at the beginning of text or after whitespace/opening punctuation (`(`, `[`, `{`, `,`, `:`, `;`), so ordinary email addresses and URL paths are not project mentions. Feelings require a word boundary before `??`; an ordinary `why??` is punctuation. Tags retain the existing explicit `#word` parsing.

While the caret is inside a mention, up to five matching saved values appear alphabetically, including values from earlier days and legacy logs. An exact normalized match needs no suggestion. **↑ / ↓** selects a suggestion, **Tab** replaces the active mention (including its suffix after the caret), and **Enter** always saves the reflection as written. Completing at the end adds a space. Suggestions temporarily replace the recall dial; **Esc** closes them without discarding text, after which arrows browse history and a further Esc follows the usual cancellation rules. New values need no separate creation step: save a reflection to add them to the vocabulary. No built-in feeling list or inferred metadata is used. Suggestion selection does not pause or extend time; accepting one is a normal edit under the same fixed typing allowance. All outputs share one suggestion selection and editor.

History lives at `$HOME/.naal-focus/activity.jsonl`, independent of XDG configuration and install prefix. `HOME` must be absolute. The directory is private (0700) and the file is private (0600), owned by the current user; symlink/hardlink targets are refused. Reflection history involves no cloud, accounts, raw key history, or writing-duration measurement. The separate opt-in focus tracker below records foreground-window time, never reflection-writing duration.

Each JSON line is a versioned record with an opaque identity, UTC Unix timestamp in milliseconds, submitted text, and explicit metadata:

```json
{"version":1,"id":"0123456789abcdef0123456789abcdef","timestamp_unix_ms":1791100000000,"text":"Reviewing @naal #rust ??focused","metadata":{"tags":["rust"],"projects":["naal"],"feelings":["focused"]}}
```

New entries append; corrections atomically replace the matching entry while preserving other records. Retries with the same identity/payload are idempotent. Earlier version-1 records without an ID or metadata remain readable and are preserved; metadata for those entries is derived from their text without rewriting the file. Corrections recompute metadata and preserve the ID/timestamp. Records are bounded at 32 KiB, including JSON overhead and metadata. Malformed, incomplete, oversized, or unsupported-version history is not silently truncated or overwritten: repair it before retrying.

## Overlay charts

In **Settings… → Overlay charts**, choose one shared **Heatmap** or **Treemap** style and switch **Projects**, **Feelings**, and **Tags** independently. Apply with **Save settings**, like other preferences. With every switch off, the existing overlay is unchanged. Turning **Enable reflections** off suppresses all of these charts without changing their saved switches or style. Existing settings without a `charts` field also default to off.

- **Heatmap**: one row per value and 24 local-hour columns, labeled `00`, `06`, `12`, `18`. Brighter cells mean more mentions in that hour; each category has its own intensity scale, stated below it. Future hours remain blank, distinct from elapsed hours with zero mentions.
- **Treemap**: labeled tile areas reflect daily mention counts, with exact counts where space allows. Nested cuts keep the tiles readable; very small tiles and long labels are clipped/shortened visually, never in storage.
- Values are ranked by today's count, with alphabetical ties. Each category shows at most four rows/tiles: if needed, the first three values plus **Other (N)**, which sums mentions of the remaining N values. One reflection may mention several values, so counts and tile areas are **not durations or exclusive shares of reflections**. Case variants and repeated mentions of the same value within one reflection count only once.
- Only saved entries from the current local day up to the load time are included initially. A successful submission updates the shared charts; correcting it replaces its original metadata counts without adding another moment. Drafts, failed saves, and merely recalling text do not change counts. Missing history is empty; loading, no-mention, and unavailable states are distinct. Midnight resets chart counts, and the next reminder reloads fresh history. Changes by external writers during a reminder are not polled.
- Enabled categories sit side by side in a reserved band above history/input, using one data snapshot across monitors. On unusually small desktop outputs, charts are omitted rather than overlapping quote/input; settings are retained. Three enabled categories fit at 800×600 logical pixels. Native keyboard, timeout, and background-click behavior are unchanged.

The optional version-1 settings field is `"charts": {"style": "heatmap", "projects": false, "feelings": false, "tags": false}`; the other supported style is `"treemap"`. Manual helper tests can use `--chart-style heatmap --charts projects,feelings,tags` (or `treemap` / `none`); **launching the helper captures focus**, so use the consented procedure in the testing guide.

## Focus history

**Settings… → Focus history → Track window time** opts in to collecting foreground application IDs and window titles through COSMIC's native window-info protocols. It is off for new and existing settings. **Graph** offers **Time spent**, **Day timeline**, and **Timeline + totals** (default); **Sidebar** offers **Left** and **Right** (default). **24-hour time** switches the sidebar clock, hourly timeline labels, and end time between 12-hour AM/PM (default, including existing settings) and 24-hour time. Compact timeline ticks use labels such as `12 PM` or `12`; complete times use `12:05 PM` or `12:05`. Duration totals and the reflection dashboard are unchanged. These preferences use the same shared settings draft/save path as other fields. Standalone helpers default to 12-hour time; `--focus-24-hour` selects 24-hour labels without enabling tracking.

- Tracking is independent of recurring reminders and snooze. Closing every applet/settings instance stops it; no separately installed service runs. Disabling tracking stops collection but keeps today's already collected records until normal day cleanup.
- One shared supervisor records the active window once across all monitors. Foreground-window time is not proof of attention. Same application ID/title pairs are combined in totals; browser tabs are distinguishable only by the title exposed by the browser. Private/incognito windows are not automatically excluded, and titles may reveal documents or conversations. No screenshots, page content, URLs beyond what appears in a title, raw keys, or input events are recorded.
- Existing guards suppress collection on lock, idle, inactive sessions, sleep, missing/stale guard observations, and supervised reminders. The existing idle threshold applies: time before the session is declared idle is not retroactively removed. Unknown/ambiguous active windows, Naal Focus's own windows, clock jumps, and gaps over three seconds are not credited. Time is accumulated using the monotonic boot clock; observations are normally within about 100 ms of a transition. Nothing before opt-in or during a closed app can be reconstructed.
- **Time spent** ranks windows by accumulated duration. At most six rows are shown (five alongside the combined timeline), with overflow summed into **Other**. **Day timeline** places recorded sessions on a vertical local-time axis, from the first recorded hour to the snapshot time. Gaps mean untracked time, not an inferred reason for being away. Short sessions remain colored but may omit labels. Colors stay consistent per application.
- Each reminder receives one in-memory snapshot shared across its outputs; its time labels and durations do not advance while the reminder is open. Midnight clears the old day. The sidebar is omitted below 960×600 logical pixels to protect quote/input, but collection continues. Existing mention charts remain in their separate band inside the remaining content area; keyboard and click behavior are unchanged.
- The only durable window-history file is `$HOME/.naal-focus/focus.json` (0600 in a 0700 directory). Writes are atomic, owner-checked, and refuse symlinks, hardlinks, and nonregular targets. Crash scratch files are cleaned on startup. Snapshots save every 15 seconds and on collection pauses; abrupt termination can lose recent unsaved time. Restarts reload today without bridging the unobserved gap. Old-day records are removed while running, even if tracking is off, or at next startup after the app has been closed. Cleanup/storage errors are reported, not treated as an empty day.
- Titles are bounded to 256 Unicode characters and application IDs to 128; control characters are removed. A day is bounded to 20,000 sessions / 32 MiB; reaching the limit pauses recording with an explicit error instead of silently dropping history. Ordinary daily usage is far smaller; rapidly changing titles can generate more sessions. This file is separate from reflections and is not included in the activity dashboard.

The optional version-1 setting is `"focus": {"enabled": false, "style": "both", "side": "right"}`. Other styles are `"totals"` and `"timeline"`; the other side is `"left"`. The helper accepts these display preferences but never starts its own tracker. Its supervisor supplies a bounded private JSON payload on stdin; titles are not placed in arguments, D-Bus status, or console logs. Legacy raw-quote `--stdin` is still supported. Use the supervised **Show now** path for consented live acceptance, not a bare helper expecting to collect history.

## Activity dashboard

**Dashboard…** in the quick-controls popup rebuilds the dashboard from the current history on every open — it is always fresh at open — writes one private HTML file beside the logs at `~/.naal-focus/dashboard.html` (0600, owner-only, with the same symlink/hardlink refusals as `activity.jsonl`), and opens it in the default browser through `xdg-open`. An already-open tab keeps its old snapshot until the dashboard is opened again or the tab is reloaded.

The page is fully self-contained: no remote assets, fonts, or network requests. Every reflection is embedded as escaped JSON and rendered as plain text, never as markup or script. Views switch in place and stay deep-linkable by hash (`#day`, `#month`):

- **Overview** — a calendar-year day map (one square per day, intensity by reflection count; days after today stay blank), weekly attention share of *tagged* reflections below it (weeks without tagged reflections are omitted, never inferred), and side panels for tag, project, and feeling counts and the day-presence streak with per-week circles, the longest run, and a days-logged fraction phrased gently. Quiet days are normal; nothing counts them as failure.
- **Day** — the day's reflections on a timeline (12-hour clock times like `11:04 AM`, or a **Relative** toggle like `43 mins ago`), with explicit quiet markers closing the gaps and a 24-hour day dial (midnight top, noon bottom, pins at true minutes, an 8 AM–6 PM band). ‹ › walks days; year-map squares open the day they represent.
- **Month** — a calendar with weekend shading, today outlined, per-day counts and tag dots, multi-tag **OR** filter chips (non-matching days dim and become unclickable), and a detail panel for the selected day.

Only documented history fields are used: counts, times of day, day presence, and explicit tags, projects, and feelings (case-insensitive, deduplicated per entry). Writing duration is never measured, so the dashboard never presents durations, hours worked, or coverage as facts; missing entries are silence, not inactivity. Tags, projects, and feelings come solely from explicit inline mentions — nothing is inferred. Project and feeling overview counts cover the saved history; day and month detail rows show all three types. Month filtering and attention-share charts continue to use tags.

**Download HTML** on the page saves a self-contained copy of the dashboard that keeps working offline, since all data is embedded. The footer notes the generation time, reflection count, and any skipped lines. Missing or empty history shows a friendly empty state explaining how the first reflection lands there — no fake numbers. Malformed history lines are skipped individually and reported in that footnote; an unreadable history opens a restrained error page instead of a misleading empty one, and the applet keeps running with its history unchanged.

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
just check-settings # Isolated settings-window process regression; opens a window
just probe          # Read-only monitor/protocol/safety check
just settings       # Open/focus settings; honors saved reminder preferences
```

`check-native` uses a private D-Bus session/runtime and never requests overlays or enables recurrence. `check-settings` additionally uses temporary preferences and opens a real settings window to check concurrent launches, reopening, and process exit without stopping the shared supervisor; it never enables reminders or requests overlays. Its private bus disables desktop-service auto-start. `probe` creates no surfaces. Run `just` to list all recipes.

See the [testing guide](TESTING.md) for coverage, recorded verification results, and manual acceptance checks. **Manual overlays capture keyboard focus**; use a disposable editor document and save other work first. A manually launched helper has no independent parent watchdog; SIGSTOP pauses its own timeout too.

## Runtime safety

See [architecture and safety rationale](ARCHITECTURE.md) for supervisor election, compositor routing, and why these safeguards are required.

- One elected applet supervises the scheduler and helper; D-Bus ownership and a process lock prevent duplicate timers.
- Lock, idle, sleep, missing guard data, and stale guard data block reminders. Checks use logind, COSMIC's runtime lock marker, and Wayland `ext_idle_notifier_v1` version 2.
- Returning from an unsafe session or a scheduling gap starts a full interval. `CLOCK_BOOTTIME` includes suspended time; missed reminders are discarded.
- The helper uses the desktop `WAYLAND_DISPLAY`, ignoring the panel's nested `WAYLAND_SOCKET`. `NAAL_FOCUS_WAYLAND_DISPLAY` can override it.
- Compositor closure, output hotplug, countdown expiry, or typing-limit expiry while writing dismisses all surfaces. Escape cancels an active draft first; without a draft it dismisses normally. Background clicks never cancel or dismiss. The supervisor kills and reaps unsafe/stalled helpers, with an independent entrance-plus-duration-plus-eight-second watchdog (plus the typing limit only when reflections are enabled) and parent-death signaling. Rendering failures pause recurrence.

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
| `src/applet.rs`, `src/applet/` | Quick popup, grouped settings draft, desktop launch and single-window activation |
| `src/config.rs` | Settings, chart/focus preferences, quote parsing/formatting, persistence |
| `src/focus.rs`, `src/focus/`, `src/render/focus.rs` | Foreground-time accounting, today-only storage, COSMIC observer, native focus sidebar |
| `src/charts.rs`, `src/render/charts.rs` | Local-day/hour mention aggregation, correction reconciliation, native heatmap/treemap layout |
| `src/activity.rs`, `src/activity_log.rs`, `src/recall.rs`, `src/metadata.rs` | Reflection editing/countdowns, private history and atomic corrections, today's grouped recall dial, shared metadata parsing and completion |
| `src/dashboard.rs`, `src/dashboard_page.html` | Pure activity-dashboard generation, private writer, detached browser open |
| `src/schedule.rs` | Scheduling, snooze, quote rotation |
| `src/engine.rs`, `src/guard.rs`, `src/process.rs` | Shared controls, safety, supervision |
| `src/overlay.rs`, `src/render.rs` | Overlay surfaces and text rendering |
| `data/`, `scripts/`, `tests/` | Packaging, assets, regression checks |
