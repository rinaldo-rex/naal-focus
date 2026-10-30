# Architecture and safety

The [app guide](html/index.html) describes app behavior. These constraints explain the process and storage boundaries:

- **One shared timer.** COSMIC can launch an applet for each panel/dock output. A session D-Bus name and runtime lock elect one supervisor; other applets and the settings window are clients, not extra reminder loops.
- **Use the desktop connection.** Panel applets can inherit a nested Wayland socket. Reminders, safety guards, and tracking must connect to the host desktop instead.
- **Independent recovery.** Reminders capture keyboard focus. The parent must stay responsive so it can kill and reap an unsafe or stalled helper independently of the helper's own deadlines. Missing or stale safety observations block reminders.
- **Bounded, nonblocking work.** History reads/writes and helper-payload transfers run on workers so storage or a stopped reader cannot block reminder input and safety checks. Reusable render buffers have memory limits so a slow compositor cannot grow an unbounded frame queue.
- **Private storage.** Preserve ownership/link checks and stable directory locks when changing history files. Corrections and focus snapshots use atomic replacement. Helper payloads use stdin; window titles must stay out of arguments, shared status, and logs.
- **No silent opt-ins.** Updates preserve preferences and keep older reflection records readable. Missing settings must not enable recurring reminders or window tracking.

## macOS prototype

The native AppKit shell shares the Rust scheduler, configuration rules, and software renderer. Linux-only dependencies and integrations stay behind target boundaries. Reflections and tracking are not wired into the Mac UI.

A background supervisor owns at most one separate reminder process. Settings transfers and heartbeats use nonblocking pipes; storage and session sampling run on separate workers. The helper has an independent startup/heartbeat/lifetime watchdog. Parent exit, stale safety, loss of focus, display changes, and workspace sleep/session notifications end reminders. Readiness requires activation, a key window, and visible windows on every selected display; no global input tap or physical display capture is used.

Lock sampling uses the macOS session dictionary’s undocumented lock-field convention. Missing console/login observations and malformed values fail closed, but absence of the lock field means unlocked under that convention. This is an experimental boundary, not a verified lock-state guarantee. Real Mac acceptance remains mandatory.

## Known limits

- Fullscreen and Do Not Disturb do not suppress reminders.
- Fractional scaling uses an integer buffer scale. The reminder uses one font without fallback or complex-script shaping; its reflection field has no clipboard integration or full IME support.
- Changes to input seats require an applet restart.
