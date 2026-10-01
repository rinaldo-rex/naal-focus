# Naal Focus

A COSMIC dock/panel applet that periodically dims all monitors with a mindfulness reminder.

**Reminders capture keyboard focus.** Press Esc or click to dismiss.

## Install

Requires a COSMIC Wayland session and Rust 1.93+. See [build prerequisites](docs/REFERENCE.md#build-prerequisites).

```sh
cargo build --locked --release --features applet
./scripts/install.sh
```

Add **Naal Focus** through **COSMIC Settings → Desktop → Dock/Panel → Applets**. Click its icon to open the controls.

## Use

- New settings default to **paused**. Enable **Recurring reminders** when ready.
- Set the interval, duration, and text size, then **Save settings**.
- Use **Snooze 15 min**, **Resume**, or **Show now** as needed.

All monitor popups share the same settings and timer. Updates preserve existing preferences.

## Quotes

Start each quote with an unindented `- `. Following lines belong to that quote:

```text
- Notice where your attention has gone.
  Take one breath.

- Return gently.
```

Line breaks and paragraphs are preserved. Add 1–100 quotes, up to 500 characters each, including line breaks.

See the [reference](docs/REFERENCE.md) for quote rules, settings limits, uninstalling, and development.
