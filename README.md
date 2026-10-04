# Naal Focus

A COSMIC dock/panel applet that periodically dims all monitors with a mindfulness reminder.

**Reminders capture keyboard focus.** Just breathe, or type an optional reflection. Esc/background click discards an active draft; otherwise it dismisses.

## Install

Requires a COSMIC Wayland session and Rust 1.93+. See [build prerequisites](docs/REFERENCE.md#build-prerequisites).

```sh
cargo build --locked --release --features applet
./scripts/install.sh
```

Add **Naal Focus** through **COSMIC Settings → Desktop → Dock/Panel → Applets**. Click its icon to open the controls.

## Use

- New settings default to **paused**. Enable **Recurring reminders** when ready.
- The compact popup offers **Show now**, **Snooze 15 min** (or **Resume** while snoozed), and the reminders toggle.
- Open **Settings…** for timing, appearance, and reflection/quotes, then **Save settings**. Reopening brings the existing window forward; closing it leaves the applet running.

All monitor popups share the same settings and timer. Updates preserve existing preferences.

## Activity reflections

The quiet textbox is ready for typing without a click. Typing holds the normal countdown; **Enter** saves locally and resumes the remaining time. Saved text replaces the textbox—click it to correct the same entry. Clicking alone does not pause the countdown; changing text does.

Set **Typing time limit** under **Settings… → Reflection** (default two minutes, adjustable from 30 seconds to ten minutes). A subtle underline shows time remaining while writing. Doing nothing preserves the normal reminder timeout.

Logs stay private at `~/.naal-focus/activity.jsonl`. Only submitted text and its timestamp are recorded; no raw keystrokes or writing duration. The dashboard is a separate, future feature. See the [activity reference](docs/REFERENCE.md#activity-reflections-and-history) for details.

## Quotes

Start each quote with an unindented `- `. Following lines belong to that quote:

```text
- Notice where your attention has gone.
  Take one breath.

- Return gently.
```

Line breaks and paragraphs are preserved. Add 1–100 quotes, up to 500 characters each, including line breaks.

See the [reference](docs/REFERENCE.md) for quote rules, settings limits, uninstalling, and development. Maintainers can start with the [architecture and safety rationale](docs/ARCHITECTURE.md) and [testing guide with verification status](docs/TESTING.md).
