# Architecture and safety

[Quick start](../README.md) · [Reference](REFERENCE.md) · [Testing](TESTING.md)

Naal Focus separates persistent controls and scheduling from the short-lived process that captures keyboard focus. It is a native COSMIC/Wayland application, not a screen lock.

## Runtime components

```mermaid
flowchart TD
    A[Applet or settings window] --> B[Shared supervisor]
    B --> C[Schedule and safety guards]
    C --> D[One-shot helper]
    D --> E[All-monitor overlays]
```

| Component | Responsibility |
|---|---|
| `naal-focus` (`src/bin/applet.rs`, `src/applet.rs`) | COSMIC dock/panel icon and compact quick controls; Settings… / `--settings` opens or activates a dedicated grouped settings window. Built with the `applet` feature. |
| `Engine` (`src/engine.rs`) | Background control client or elected supervisor. Publishes shared status, applies requests, and owns at most one helper. |
| `Schedule` (`src/schedule.rs`) | Pure interval, snooze, and sequential quote-rotation policy. |
| `Guard` (`src/guard.rs`) | Checks session state through system D-Bus/logind and input idle state through Wayland. |
| Focus tracker (`src/focus.rs`, `src/focus/`) | Opt-in supervisor-owned COSMIC window observer, monotonic interval accounting, and one private current-day file. |
| `naal-focus-overlay` (`src/bin/overlay.rs`, `src/overlay.rs`) | Displays one reminder using one layer-shell surface per selected output. Has no recurring timer. |
| Renderer (`src/render.rs`) | Rasterizes text into shared-memory ARGB8888 buffers using a local font; caches the quote and repaints only the history/input strip during editing and dial motion. |
| Reflection state (`src/activity.rs`) | Pure UTF-8 editor, held/resumed countdown, fixed typing allowance, and accepted-text/correction state. |
| Activity history (`src/activity_log.rs`) | Private JSON-lines records, durable worker acknowledgments, idempotent retries, and atomic corrections. |
| Metadata (`src/metadata.rs`) | Shared explicit mention parsing, normalized per-kind values, and history vocabulary lookup. |
| Overlay charts (`src/charts.rs`, `src/render/charts.rs`) | Shared local-day/hour mention counts and cached native heatmap/treemap rendering. |
| Reflection recall (`src/recall.rs`) | Read-only worker loads today's records and the archive vocabulary; pure consecutive grouping, keyboard dial selection, relative labels, and shared rolling animation. |
| Dashboard (`src/dashboard.rs`, `src/dashboard_page.html`) | Pure generation of the self-contained activity page, private writer, and detached browser open. |
| Settings (`src/config.rs`) | Validates preferences and quote syntax; loads and atomically saves JSON. |
| Process boundaries (`src/process.rs`) | Desktop compositor routing, runtime locks, boot-time clock, and helper cleanup. |

The supervisor sends the selected quote and optional focus-history snapshot as bounded JSON on stdin, never placing private text in process arguments. A dedicated writer thread prevents a stopped pipe reader from blocking the supervisor's safety watchdog. The helper validates the entire payload before creating any surfaces; legacy raw-quote `--stdin` remains supported. The helper creates coordinated overlays with one shared reflection state. Reflection history contains only Enter-submitted text and its explicit metadata; raw keys are never recorded. Automatic window history is a separate, off-by-default opt-in described below. Escape cancels a draft before ordinary dismissal; background clicks never cancel or dismiss; timeout, compositor closure, or output hotplug dismisses all surfaces.

## Reflection state and persistence

The field is initially ready for input, without holding the normal countdown. A text change or explicit history browsing freezes the remaining duration and starts the typing allowance once. Successful save or cancellation resumes exactly that duration. Accepted text replaces the field; clicking reopens it without pausing until the text changes. Corrections retain the record's ID and original timestamp. The typing deadline is never renewed; it applies while writing or browsing, not to the breathing countdown after a successful submission.

The helper owns the log worker; there is no new supervisor storage protocol. One in-flight submission prevents duplicate Enter events. Enter on accepted text reopens the shared editor at the end without pausing the countdown; only a text change starts correction editing. Enter on a reopened unchanged editor closes it without writing. Both Return and keypad Enter are routed before the hidden-field guard, while held-key repeats remain ignored. The worker writes privately under `~/.naal-focus/` and acknowledges only after syncing. A stable advisory directory lock serializes append and correction operations even when the data-file inode changes. Corrections stream a validated history to a private temporary file, sync it, atomically rename it over the original, then sync the directory. Stable opaque IDs make acknowledgment retries idempotent; stale corrections fail without replacing newer data. History scanning bounds each record and does not load the whole file into memory.

Generation tokens keep late save results from erasing a newer draft. Canceling does not undo a write already explicitly submitted. The input/render loop only polls the worker and continues checking safety; a stalled filesystem must not strand keyboard focus.

At reminder opening, a separate worker scans history with the existing read-only owner/link/lock guards, filtering while scanning so only current-local-day records are retained. Future timestamps are excluded; valid records are stably sorted by timestamp and only consecutive identical texts are grouped. Missing history is empty; unreadable/busy history leaves input and safety checks active with an unavailable hint. The same scan collects unique tag/project/feeling vocabulary from all nonfuture valid records, while retaining only today’s full entries. The resulting snapshot is shared across all outputs. Browsing never modifies the editor; exiting restores its caret/selection and prior writing state. Enter recalls into the editor, not persistence; the next Enter uses the existing new-entry submission path. Corrections still apply only to this reminder's saved entry, never the recalled historical source.

Live metadata pills and caret-local completion use the same parser as persistence and the dashboard. Suggestions replace the history area temporarily; arrows select, Tab inserts, Escape closes suggestions, and Enter retains its save behavior. Completion uses the editor’s existing character bound and fixed typing deadline. New records carry normalized metadata alongside untouched text; old records derive it on read. Corrections recalculate it, and readers/writers reject metadata that disagrees with its text.

Each output reuses two buffers with identical cached quote content. The reserved history/input/status strip is repainted and damaged for text changes, dial animation, caret blinking, or progress; quote pixels remain cached and outside that damage. When charts are enabled, a separate band above that strip is reserved during quote layout. Each buffer tracks its chart revision: history completion, successful saves/corrections, and local hour/day rollover repaint and damage that band, but ordinary typing and caret motion do not rerasterize charts. The seeded second buffer inherits the first buffer's revision. The same monotonic animation time is used across outputs. Both buffers are initialized before normal typing begins; a compositor that stops releasing them cannot cause an unbounded allocation queue.

## Overlay charts

Chart preferences default to all categories off, preserving old version-1 settings. One shared style (hourly heatmap or daily treemap) and three independent category switches travel through existing settings synchronization and explicit helper CLI arguments; no new service or persistence format is introduced.

The recall worker builds chart counts from the same filtered, read-only local-day snapshot before grouping consecutive reflection texts. Thus repeated saved moments remain separate counts. Normalized metadata is deduplicated per value per entry; no draft text or inferred duration contributes. Compact per-ID observations support replacing earlier metadata counts on correction. Reconciling this reminder's latest acknowledged entry into a late history result is an upsert, so a snapshot already containing it does not count it twice. Legacy ID-less records are counted independently. The renderer uses a bounded top-four view, merging overflow into Other without discarding mentions.

Charts are noninteractive and share one aggregate snapshot across outputs. Missing history renders empty, while pending and failed reads get explicit states. Local hour rollover reveals the next heatmap column; local day rollover clears yesterday's counts. A reminder is a snapshot plus its own acknowledged writes, not a live subscription to external history writers. Unusually small desktop outputs omit the band to protect the existing quote and reflection controls. None of these states changes focus, key routing, dismissal, or watchdog policy.

## Foreground-window history

The elected supervisor owns exactly one tracker worker, independent of the reminder enabled/snooze state. A second process lock covers the worker lifetime and final flush across supervisor takeover. Other panel instances relay only preferences and a generic focus-error status; window titles never enter the shared D-Bus status payload. No compositor window-list subscription is created until tracking is enabled.

The worker connects to the validated desktop socket, not the nested panel socket. It combines `ext_foreign_toplevel_list_v1` identity/title events with `zcosmic_toplevel_info_v1` version 2 activation state. Unknown or multiple active windows are gaps, never simultaneous measured work. Application IDs/titles are bounded and sanitized before retention; only the selected foreground window is persisted. Loss of either protocol makes tracking unavailable and retries without affecting reminders.

The supervisor publishes a short-lived collection permission/heartbeat from the existing safety guards, suppressing accounting before spawning a helper and throughout its lifetime. A stale heartbeat, a worker scheduling gap, suspend, unsafe session, missing active window, or wall-clock discontinuity discards the uncertain interval. `Clock` uses monotonic durations with fractional carry for fast event streams; wall timestamps only locate sessions on today's timeline. Consecutive intervals with the same application/title merge. New processes never bridge the unobserved restart gap.

The worker maintains a shared in-memory `Day`; the supervisor clones it once per reminder. Disk I/O and JSON pipe serialization never hold the supervisor/render/input path. One owner-checked `focus.json` snapshot is atomically replaced every 15 seconds and when collection pauses. The activity directory's existing advisory lock serializes replacement against reflection operations. Only the current local date is retained; startup/day rollover remove stale records even when collection is disabled. Reserved crash scratch files are removed under the same lock. If the application is closed at midnight, cleanup necessarily waits for its next startup. Storage/cleanup failures remain explicit errors, separate from protocol failures.

The native sidebar is noninteractive. A horizontal content viewport reserves its space consistently for quote layout, mention charts, reflection rendering, surface damage, and pointer coordinates. Existing charts and input cannot repaint the sidebar. Both render buffers cache the same snapshot; shared chart revisions refresh it only when needed, not on ordinary typing/caret motion. All three styles and both sides share the same bounded data and per-application colors. Small desktop outputs omit only the column, not collection or reminder controls. No focus durations are inferred for reflections or added to the dashboard.

## Activity dashboard

The dashboard is generated, not served: **Dashboard…** rescans the history on every open through the lenient per-line loader, renders one self-contained HTML page, writes it privately beside the logs, and opens it through `xdg-open`. No server runs, and no history data is passed through process arguments or the network.

Generation is a pure module — parsed entries, one timestamp, and a time-zone conversion in; a single HTML string out — fully unit-testable without the GUI. The page embeds the aggregated data as JSON inside an inert `<script type="application/json">` element with `<`, `&`, and the JS line separators escaped into JSON escapes, so reflection text can never close the element or become executable markup. All rendering and interactivity happens client-side in the embedded script, built exclusively through DOM APIs that treat text as text.

The applet only polls the dashboard worker from its existing tick; blocking filesystem work never freezes the popup controls. Failures surface as the popup status line, never as a crash of the panel process. The output file is written privately (0600, owner-only) with the same symlink/hardlink refusals as `activity.jsonl`, replacing any earlier generation in place.

Statistics claim nothing the history does not contain: counts, times of day, day presence, and explicit tag/project/feeling vocabulary derive from timestamps and text; durations and hours worked are never claimed because writing duration is never measured. Weeks run Monday through Sunday; the year map covers the current calendar year; days after today render as blank squares rather than zero counts. Malformed lines are skipped individually and reported in a quiet footnote; an unreadable history renders a restrained error page instead of a misleading empty one.

## One timer across multiple instances

COSMIC can launch an applet instance for each panel/dock output. The standalone settings window is another client; it must not introduce another reminder loop.

Each engine tries to own the session D-Bus name `io.github.rinaldo_rex.NaalFocus.Control` without queueing. The winner runs the supervisor and holds the `naal-focus-applet.lock` process lock in `XDG_RUNTIME_DIR`. Other instances relay requests and poll the owner's status; they do not start their own timer or guards.

The control object is `/io/github/rinaldo_rex/NaalFocus/Control`, with interface `io.github.rinaldo_rex.NaalFocus.Control1`. `Snapshot` returns JSON status and `Send` accepts a JSON request. Remote requests are size-bounded and validated; they cannot request supervisor shutdown.

- Closing a secondary instance only disconnects that client.
- Closing the owner drops its helper and releases ownership. Surviving clients compete to become the replacement.
- The replacement reloads saved settings, carries over the remaining snooze from its last status snapshot, and starts a fresh interval after safety checks.
- A brief process-lock overlap during replacement is retried instead of permanently disconnecting the controls.
- Closing every instance stops reminders. There is no separately installed daemon or service, and snoozes do not persist across a complete shutdown.

UI status is sampled rather than instantaneous: the owner processes its loop about every 100 ms, clients poll about every 250 ms, and controls refresh once per second. Settings drafts retain unsaved edits when shared settings change, including after a failed save or typing after a save request. The draft becomes clean when shared status matches the requested settings without an error.

## Quick controls and settings window

The popup contains only status, immediate reminder controls, and a Settings… entry point. The separate resizable desktop window groups configuration into Timing, Appearance, Overlay charts, Focus history, and Reflection, with a native quote editor and explicit Save.

`src/applet/launch.rs` launches the same binary with `--settings`, detached from the panel lifetime. It removes nested/privileged Wayland socket variables, resets panel scaling and stale launch-token overrides, and routes `WAYLAND_DISPLAY` to the validated desktop socket. COSMIC's applet-token subscription obtains a host-compositor activation token through the privileged channel; a panel-local token cannot authorize desktop focus. The window consumes the startup token after opening, while libcosmic handles tokens forwarded over its D-Bus activation interface for an existing window.

A separate `naal-focus-settings-window.lock` in `XDG_RUNTIME_DIR` serializes startup and the window lifetime, closing the race between simultaneous launches from different panels before D-Bus registration. A losing launcher retries activation for up to five seconds rather than creating a duplicate window. This window lock is independent of the shared-supervisor lock; closing settings does not shut down another applet's supervisor. If settings is the only remaining client, the existing all-instances-closed rule still applies.

## Desktop versus panel compositor

The applet UI runs through COSMIC's panel connection. That connection is not the desktop compositor: applets can inherit a nested `WAYLAND_SOCKET` which cannot create desktop-wide overlays.

`desktop_connection()` therefore connects directly to the socket named by `NAAL_FOCUS_WAYLAND_DISPLAY`, or otherwise `WAYLAND_DISPLAY`. Relative socket names resolve under `XDG_RUNTIME_DIR`; absolute socket paths are accepted. The installed desktop entry requests the host display through `X-HostWaylandDisplay=true`.

The supervisor validates this route before spawning the helper, removes `WAYLAND_SOCKET` and `X_PRIVILEGED_WAYLAND_SOCKET` from its environment, and passes the resolved desktop socket in `NAAL_FOCUS_WAYLAND_DISPLAY`. Both the idle guard and overlay use this explicit desktop connection. No display name is hardcoded.

## Safety rationale

Because reminders intentionally capture focus, uncertainty must suppress them rather than risk leaving an input-capturing overlay on an unsafe session.

| Policy | Mechanism and reason |
|---|---|
| Recurrence is opt-in | New settings start paused; installing or opening controls must not silently start interruptions. |
| Missing or stale safety data blocks reminders | Session and idle observations expire after three seconds. A disconnected or stalled guard must not be interpreted as an unlocked, active desktop. |
| Lock, idle, inactive sessions, and sleep block reminders | Session polling checks logind properties and COSMIC's runtime lock marker. Wayland `ext_idle_notifier_v1` version 2 observes input idle even when another application inhibits idle. No inhibitor is requested. |
| Unsafe sessions and scheduling gaps reset the interval | `CLOCK_BOOTTIME` includes suspended time. Gaps over three seconds and safety recovery start a full interval, discarding missed reminders instead of catching up immediately after wake or unlock. |
| Only one reminder is active | The supervisor owns at most one child, and the helper holds a separate overlay process lock. Manual requests cannot bypass safety or overlap an existing reminder. |
| Helper lifetime is bounded | The renderer handles its normal countdown, fixed typing allowance, and hard deadline. The parent independently checks safety and a duration-plus-typing-limit-plus-eight-second deadline, kills and reaps the child on cancellation, and sets Linux parent-death signaling. A stopped helper cannot defeat the parent's watchdog. |
| Failed helpers pause recurrence | Launch or unsuccessful helper-exit failures pause recurring reminders rather than repeatedly reopening a broken focus-capturing process. This is not a guarantee that the underlying failure has been repaired. |
| Invalid settings are preserved | Invalid or unsupported settings fall back to paused defaults. Only an explicit Save replaces the original file, so merely toggling recurrence cannot destroy it. |

The helper also checks safety independently, requires outputs to configure within three seconds, and caps render-buffer sizes. Saving settings or snoozing cancels a pending/visible reminder. **Show now** waits two seconds, including when recurrence is paused, but still requires a safe session.

These are safeguards, not a security boundary. Compositor shortcuts can still work, and restoring focus is COSMIC's responsibility. A manually launched helper has no parent supervisor; stopping it with SIGSTOP also stops its in-process timeout and watchdog. See the reference for rendering and session limitations.
