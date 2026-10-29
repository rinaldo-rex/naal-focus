# Naal Focus

A COSMIC dock/panel applet that periodically dims all monitors with a mindfulness reminder.

Going forward, the [HTML app guide](docs/html/index.html) is the go-to documentation and source of truth for app behavior and UI intentions. Keep it updated when the app changes.

**Reminders capture keyboard focus.** Just breathe, or type an optional reflection. Esc discards an active draft; otherwise it dismisses. Background clicks do nothing.

## Install

Requires a COSMIC Wayland session and Rust 1.93+. See [build prerequisites](docs/REFERENCE.md#build-prerequisites).

```sh
cargo build --locked --release --features applet
./scripts/install.sh
```

Add **Naal Focus** through **COSMIC Settings → Desktop → Dock/Panel → Applets**. Click its icon to open the controls.

## Use

- New settings default to **paused**. Enable **Recurring reminders** when ready.
- The compact popup offers **Show now**, **Snooze 15 min** (or **Resume** while snoozed), **Dashboard…**, and the reminders toggle.
- Open **Settings…** for timing, appearance, and reflection/quotes, then **Save settings**. Reopening brings the existing window forward; closing it leaves the applet running.

All monitor popups share the same settings and timer. Updates preserve existing preferences.

## Breathing entrance

Each reminder gently fades in with a fresh seeded pattern: **Radial lattice**, **Nested polygons**, **Constellation**, **Woven field**, **Ember bloom**, **Moon pool**, or **Glass jelly**. The three organic studies flow gently as they breathe. All monitors share the same pattern and breath. By default, one breath (4 seconds in, 6 out) comes before the quote, history, charts, and textbox appear. The full entrance takes 14 seconds; your reflection countdown starts afterward.

Under **Settings… → Breathing**, switch breathing on/off, choose a pace and one or two breaths, and select the allowed pattern families. Keep at least one selected. **Reduced motion** holds the geometry still with timed breath cues and gentle fades. Typing during the entrance is ignored; **Esc** immediately dismisses all monitors. New or missing breathing settings include all seven patterns; saved pattern selections are kept. Recurring reminders are never enabled by this update.

## Activity reflections

Under **Settings… → Reflection**, turn **Enable reflections** off to hide the textbox, past reflections, metadata/suggestions, typing hints, and reflection charts. Breathing, quotes, and the optional window-time sidebar remain; typing is ignored and **Esc** dismisses. Saved history and chart preferences are kept. Reflections default to **on**, including existing settings.

After the entrance, the quiet textbox is ready for typing without a click. Typing holds the normal countdown; **Enter** saves locally and resumes the remaining time. Saved text replaces the textbox—press **Enter again** or click it to correct the same entry. Enter reopens with the cursor at the end. Reopening alone does not pause the countdown; changing text does.

Add `#tags`, `@projects`, and `??feelings` inline (for example, `Reviewing @naal #rust ??focused`). Pills appear above the textbox as you type. Existing values autocomplete from your saved history: **↑ / ↓** select, **Tab** inserts, and **Enter** saves the text as written. **Esc** closes suggestions first; history browsing returns when suggestions are closed. Values are case-insensitive and may use letters, numbers, `_`, and `-`.

Three faded rows above the textbox show today's previous reflections, grouping consecutive repeats. **↑ / ↓** roll through the day's history without replacing your draft; only the highlighted row shows its age and repeat count. **Enter** copies the selected text into the textbox; edit if needed, then **Enter again** saves a new moment. **↓ past the newest** or **Esc** returns to your draft. Browsing shares the fixed typing allowance; simply displaying history does not pause the countdown.

Set **Typing time limit** under **Settings… → Reflection** (default two minutes, adjustable from 30 seconds to ten minutes). A subtle underline shows time remaining while writing. Doing nothing preserves the normal reminder timeout.

Logs stay private at `~/.naal-focus/activity.jsonl`. Only submitted text, its explicit metadata, and its timestamp are recorded; no raw keystrokes or writing duration.

Open **Dashboard…** from the popup for a local view of that history: a self-contained HTML page regenerated fresh on every open, with a year day map, day timeline with a 24-hour dial, and a month calendar with tag filtering. Project and feeling counts appear in the overview; all three metadata types appear beside reflections. It stays offline and private — no remote assets, escaped text only — and **Download HTML** on the page saves an offline copy. See the [activity reference](docs/REFERENCE.md#activity-dashboard) for details.

## Overlay charts

Under **Settings… → Overlay charts**, choose **Heatmap** (hourly mentions) or **Treemap** (daily totals), then enable **Projects**, **Feelings**, and/or **Tags**. One style applies to all enabled categories; charts start **off**, including when upgrading.

Charts show today's saved mentions, not time spent. Successful saves and corrections update them across monitors; drafts do not. Up to four values are shown per category, grouping overflow as **Other**. Charts sit above the history/input area without changing keyboard controls.

## Focus history

Under **Settings… → Focus history**, enable **Track window time**. Tracking starts **off**, including on upgrades. Choose **Time spent**, **Day timeline**, or **Timeline + totals** (default), and a **Left** or **Right** sidebar (default Right). Times use a 12-hour clock with AM/PM by default; enable **24-hour time** to switch the sidebar clock and timeline labels.

This records foreground applications and window titles while the applet runs—not screenshots, keystrokes, or proof of attention. Tracking keeps running when reminders are paused or snoozed; its own switch stops collection. Idle, lock, sleep, missing observations, Naal Focus windows, and supervised reminder time are excluded.

Only today's history is kept privately in `~/.naal-focus/focus.json`. Older titles are cleared at day rollover while running, or on the next startup if the app was closed. Browser/document titles can be sensitive, including private-window titles. No data leaves the device. See the [focus-history reference](docs/REFERENCE.md#focus-history) for timing, storage limits, and display behavior.

## Quotes

Start each quote with an unindented `- `. Following lines belong to that quote:

```text
- Notice where your attention has gone.
  Take one breath.

- Return gently.
```

Line breaks and paragraphs are preserved. Add 1–100 quotes, up to 500 characters each, including line breaks.

See the [reference](docs/REFERENCE.md) for quote rules, settings limits, uninstalling, and development. Maintainers can start with the [architecture and safety rationale](docs/ARCHITECTURE.md) and [testing guide with verification status](docs/TESTING.md).
