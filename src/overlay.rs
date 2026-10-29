//! One coordinated, short-lived native overlay on every selected output.
use crate::{
    activity::{Activity, Expiry},
    activity_log::{Entry, PendingSave},
    breathing::Entrance,
    config::{ChartSettings, ChartStyle, FocusSide, FocusStyle, Settings, validate_quotes},
    dashboard, focus,
    guard::Guard,
    process::{Singleton, boot_time, desktop_connection},
    recall::{self, PendingHistory},
    render,
};
use fontdue::Font;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::{collections::HashMap, error::Error, io::Read, time::Duration};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

pub const HELP: &str = "Naal Focus — one-shot overlay helper for COSMIC/Wayland

Usage: naal-focus-overlay [OPTIONS]
  --check              Read-only protocol, monitor, and session safety probe
  --once               Show one reminder (default); never runs its own timer
  --all                Cover all monitors (default)
  --output NAME        Cover one named monitor for manual testing
  --duration-secs N    Normal visible countdown, 1–30 (default 5)
  --reflections MODE   on (default) or off; off hides writing, history, and charts
  --typing-limit-secs N  Typing deadline, 30–600 (default 120)
  --delay-secs N       Delay before showing, 0–60 (default 0)
  --text-size N        Logical text size, 22–72 (default 36)
  --idle-secs N        Skip idle sessions, 30–3600 (default 120)
  --chart-style STYLE  heatmap (hourly) or treemap (daily totals)
  --charts KINDS       Comma-separated projects,feelings,tags or none (default)
  --focus-history      Show collected focus history; does not start tracking
  --focus-style STYLE  totals, timeline, or both (default)
  --focus-side SIDE    left or right (default)
  --focus-24-hour      Use 24-hour sidebar times (default: 12-hour AM/PM)
  --breathing MODE     on (default) or off; entrance precedes reflection
  --breath-pace PACE   short (3/4), balanced (4/6, default), even (5/5)
  --breath-cycles N    One or two breaths (default 1)
  --breath-patterns LIST  lattice,polygons,constellation,weave (default all)
  --reduced-motion    Static geometry with timed breath cues
  --payload            Read the supervisor's bounded JSON quote/history payload
  --stdin              Read one raw UTF-8 quote (including line breaks) from stdin
  -h, --help           Print help

WARNING: captures keyboard focus on all covered monitors. Type to reflect;
During the breathing entrance or with reflections off, typing is ignored and Escape dismisses all.
With reflections on after the entrance, Up/Down browse history; Enter recalls, then saves.
Use #tags, @projects, and ??feelings; arrows select suggestions, Tab inserts.
After saving, Enter reopens the reflection for correction (or click it).
With suggestions open, Enter saves as written; Escape closes suggestions first.
Typing or browsing holds the normal countdown up to the typing limit. Escape
while browsing returns to the draft; while typing discards and resumes;
otherwise it dismisses ALL
surfaces. Background clicks do nothing. No raw keys are logged. This is not a screen lock.
";

#[derive(Debug, Default)]
pub struct Options {
    pub settings: Settings,
    pub check: bool,
    pub delay: u64,
    pub output: Option<String>,
    pub stdin: bool,
    pub payload: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    pub quote: String,
    pub focus: Option<focus::Snapshot>,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut options = Self::default();
        let mut all = false;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--check" => options.check = true,
                "--once" => {}
                "--all" => all = true,
                "--stdin" => options.stdin = true,
                "--payload" => options.payload = true,
                "--focus-history" => options.settings.focus.enabled = true,
                "--focus-24-hour" => options.settings.focus.use_24_hour_time = true,
                "--reflections" => {
                    options.settings.reflections_enabled = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--reflections requires on or off.".into()),
                    };
                }
                "--breathing" => {
                    options.settings.breathing.enabled = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--breathing requires on or off.".into()),
                    };
                }
                "--breath-pace" => {
                    options.settings.breathing.pace = match args.next().as_deref() {
                        Some("short") => crate::breathing::Pace::Short,
                        Some("balanced") => crate::breathing::Pace::Balanced,
                        Some("even") => crate::breathing::Pace::Even,
                        _ => return Err("--breath-pace requires short, balanced, or even.".into()),
                    };
                }
                "--breath-cycles" => {
                    options.settings.breathing.cycles = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Choose one or two breaths.")?;
                }
                "--breath-patterns" => {
                    let list = args.next().ok_or("--breath-patterns requires a list.")?;
                    options.settings.breathing.families = list
                        .split(',')
                        .map(|name| {
                            crate::breathing::Family::ALL
                                .into_iter()
                                .find(|f| f.as_str() == name)
                                .ok_or_else(|| format!("Unknown breathing pattern: {name}"))
                        })
                        .collect::<Result<_, _>>()?;
                }
                "--reduced-motion" => options.settings.breathing.reduced_motion = true,
                "--focus-style" => {
                    options.settings.focus.style = match args.next().as_deref() {
                        Some("totals") => FocusStyle::Totals,
                        Some("timeline") => FocusStyle::Timeline,
                        Some("both") => FocusStyle::Both,
                        _ => return Err("--focus-style requires totals, timeline, or both.".into()),
                    };
                }
                "--focus-side" => {
                    options.settings.focus.side = match args.next().as_deref() {
                        Some("left") => FocusSide::Left,
                        Some("right") => FocusSide::Right,
                        _ => return Err("--focus-side requires left or right.".into()),
                    };
                }
                "--output" => {
                    options.output = Some(
                        args.next()
                            .filter(|v| !v.is_empty() && !v.starts_with('-'))
                            .ok_or("--output requires a monitor name.")?,
                    )
                }
                "--chart-style" => {
                    options.settings.charts.style = match args.next().as_deref() {
                        Some("heatmap") => ChartStyle::Heatmap,
                        Some("treemap") => ChartStyle::Treemap,
                        _ => return Err("--chart-style requires heatmap or treemap.".into()),
                    };
                }
                "--charts" => {
                    let value = args.next().ok_or("--charts requires categories or none.")?;
                    let mut charts = ChartSettings {
                        style: options.settings.charts.style,
                        ..ChartSettings::default()
                    };
                    if value != "none" {
                        for category in value.split(',') {
                            match category {
                                "projects" => charts.projects = true,
                                "feelings" => charts.feelings = true,
                                "tags" => charts.tags = true,
                                _ => {
                                    return Err(
                                        "--charts requires projects,feelings,tags or none.".into(),
                                    );
                                }
                            }
                        }
                    }
                    options.settings.charts = charts;
                }
                "--duration-secs" => options.settings.duration_secs = number(&arg, args.next())?,
                "--typing-limit-secs" => {
                    options.settings.typing_limit_secs = number(&arg, args.next())?
                }
                "--delay-secs" => options.delay = number(&arg, args.next())?,
                "--text-size" => {
                    options.settings.text_size = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Text size is too large.")?
                }
                "--idle-secs" => {
                    options.settings.idle_secs = number(&arg, args.next())?
                        .try_into()
                        .map_err(|_| "Idle threshold is too large.")?
                }
                _ => return Err(format!("Unknown option {arg}; use --help.")),
            }
        }
        options.settings.validate()?;
        if options.stdin && options.payload {
            return Err("Choose --stdin or --payload, not both.".into());
        }
        if options.delay > 60 {
            return Err("Delay must be 0–60 seconds.".into());
        }
        if all && options.output.is_some() {
            return Err("Choose --all or --output, not both.".into());
        }
        Ok(Some(options))
    }
}
fn number(option: &str, text: Option<String>) -> Result<u64, String> {
    text.ok_or_else(|| format!("{option} requires an integer."))?
        .parse()
        .map_err(|_| format!("{option} requires an integer."))
}

fn read_stdin_quote(input: impl Read) -> Result<String, String> {
    let mut quote = String::new();
    input
        .take(4097)
        .read_to_string(&mut quote)
        .map_err(|e| e.to_string())?;
    if quote.len() > 4096 {
        return Err("Quote input is too large.".into());
    }
    // This is already one selected quote, not the bulk-editor format. Parsing
    // bullet delimiters here would split or corrupt literal lists in its text.
    validate_quotes(std::slice::from_ref(&quote))?;
    Ok(quote)
}

fn read_payload(input: impl Read) -> Result<Payload, String> {
    let limit = focus::MAX_BYTES + 8192;
    let mut bytes = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Overlay payload is too large.".into());
    }
    let mut payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid overlay payload.")?;
    validate_quotes(std::slice::from_ref(&payload.quote))?;
    if let Some(snapshot) = &mut payload.focus {
        snapshot.day.validate(recall::system_ms())?;
        snapshot.day.advance_day(recall::system_ms());
    }
    Ok(payload)
}

pub fn run(mut options: Options) -> Result<(), Box<dyn Error>> {
    let focus_snapshot = if options.payload && !options.check {
        let payload = read_payload(std::io::stdin())?;
        options.settings.quotes = vec![payload.quote];
        payload.focus
    } else {
        None
    };
    if options.stdin && !options.check {
        options.settings.quotes = vec![read_stdin_quote(std::io::stdin())?];
    }
    let conn = desktop_connection()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("Desktop does not expose wlr-layer-shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh)?;
    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        pool: SlotPool::new(4096, &shm)?,
        shm,
        keyboards: HashMap::new(),
        pointers: HashMap::new(),
        active: Vec::new(),
        reason: None,
        failure: None,
        font: None,
        quote: options.settings.quotes[0].clone(),
        text_size: options.settings.text_size,
        reflections_enabled: options.settings.reflections_enabled,
        charts: options.settings.overlay_charts(),
        chart_revision: 1,
        focus_settings: options.settings.focus.clone(),
        focus_snapshot,
        entrance: Entrance::new(options.settings.breathing.clone(), random_seed()?),
        requested_at: None,
        mapped_at: None,
        activity: Activity::new(
            Duration::from_secs(options.settings.duration_secs),
            Duration::from_secs(options.settings.typing_limit_secs),
        ),
        save: None,
        pending_history: None,
        history_unavailable: false,
        saved_entry: None,
        retry_entry: None,
        modifiers: HashMap::new(),
        loop_handle: event_loop.handle(),
        dirty: false,
        last_paint: Duration::ZERO,
    };
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    let mut selected = Vec::new();
    for output in app.outputs.outputs() {
        if let Some(info) = app.outputs.info(&output) {
            let name = info.name.as_deref().unwrap_or("(unnamed)");
            println!(
                "Output: {name} · logical {:?} · scale {}",
                info.logical_size, info.scale_factor
            );
            if options.output.as_ref().is_none_or(|wanted| wanted == name) {
                selected.push(output);
            }
        }
    }
    if selected.is_empty() {
        return Err("No matching outputs; use --check to list monitors.".into());
    }
    let guard = Guard::start(options.settings.idle_secs);
    let wait_started = boot_time();
    loop {
        let status = guard.snapshot();
        if status.session_seen.is_some() && status.idle_seen.is_some() {
            let blocked = status.blocked_reason(boot_time());
            println!(
                "Session guard: {}",
                blocked
                    .as_deref()
                    .unwrap_or("ready (active, unlocked, not idle)")
            );
            if options.check {
                let protocols = globals.contents().clone_list();
                let focus_available = protocols
                    .iter()
                    .any(|g| g.interface == "zcosmic_toplevel_info_v1" && g.version >= 2)
                    && protocols
                        .iter()
                        .any(|g| g.interface == "ext_foreign_toplevel_list_v1");
                println!(
                    "Focus history protocols: {} (no window metadata requested).",
                    if focus_available {
                        "available"
                    } else {
                        "unavailable"
                    }
                );
                println!(
                    "Desktop layer-shell and shared-memory rendering available; no surfaces created."
                );
                return Ok(());
            }
            if let Some(reason) = blocked {
                return Err(reason.into());
            }
            break;
        }
        if boot_time() - wait_started > Duration::from_secs(3) {
            return Err(status
                .blocked_reason(boot_time())
                .unwrap_or_else(|| "Guard startup timed out.".into())
                .into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _singleton = Singleton::acquire("overlay")?;
    app.font = Some(render::default_font()?);
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle())?;
    let show_at = boot_time() + Duration::from_secs(options.delay);
    loop {
        let now = boot_time();
        if let Some(reason) = guard.snapshot().blocked_reason(now) {
            app.active.clear();
            conn.flush()?;
            println!("Reminder skipped/cancelled: {reason}");
            return Ok(());
        }
        if app.requested_at.is_none() && now >= show_at {
            app.show(&qh, &selected);
            arm_watchdog(options.settings.helper_lifetime_secs());
        }
        let interval = if app.entrance_frame(now).active {
            16
        } else {
            50
        };
        event_loop.dispatch(Duration::from_millis(interval), &mut app)?;
        let now = boot_time();
        app.poll_save(now);
        app.poll_history();
        if let Some(expiry) = app.activity.expired(now) {
            app.reason = Some(match expiry {
                Expiry::Normal => "timeout",
                Expiry::Typing => "typing-limit",
            });
        }
        if app
            .requested_at
            .is_some_and(|at| boot_time().saturating_sub(at) >= Duration::from_secs(3))
            && app.active.iter().any(|a| !a.mapped)
        {
            app.failure = Some("Not every monitor configured within 3 seconds.".into());
        }
        if let Some(error) = app.failure.take() {
            app.active.clear();
            conn.flush()?;
            return Err(error.into());
        }
        if app.reason.is_none() && app.failure.is_none() {
            app.repaint(now)?;
        }
        if let Some(reason) = app.reason.take() {
            println!(
                "Dismissed ALL outputs: {reason} · mapped={} · keyboard-focus-observed={}",
                app.active.iter().filter(|a| a.mapped).count(),
                app.active.iter().any(|a| a.keyboard_focus_observed)
            );
            app.active.clear();
            conn.flush()?;
            return Ok(());
        }
    }
}

fn random_seed() -> Result<u64, std::io::Error> {
    let mut bytes = [0; 8];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(u64::from_ne_bytes(bytes))
}

fn arm_watchdog(seconds: u64) {
    let deadline = boot_time() + Duration::from_secs(seconds);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            if boot_time() >= deadline {
                eprintln!("Overlay hard deadline exceeded; terminating to release focus.");
                std::process::exit(1);
            }
        }
    });
}

struct Active {
    layer: LayerSurface,
    output: wl_output::WlOutput,
    size: (u32, u32),
    scale: i32,
    mapped: bool,
    keyboard_focus_observed: bool,
    buffers: Vec<Buffer>,
    chart_versions: Vec<u64>,
    entrance_dirty: Vec<bool>,
    entrance_backdrops: Vec<[u8; 4]>,
    reveal_cache: Option<Vec<u8>>,
    buffer_size: (u32, u32, u32),
}
struct SaveJob {
    generation: u64,
    entry: Entry,
    result: PendingSave,
}
struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    keyboards: HashMap<wl_seat::WlSeat, wl_keyboard::WlKeyboard>,
    pointers: HashMap<wl_seat::WlSeat, wl_pointer::WlPointer>,
    active: Vec<Active>,
    reason: Option<&'static str>,
    failure: Option<String>,
    font: Option<Font>,
    quote: String,
    text_size: u32,
    reflections_enabled: bool,
    charts: ChartSettings,
    chart_revision: u64,
    focus_settings: crate::config::FocusSettings,
    focus_snapshot: Option<focus::Snapshot>,
    entrance: Entrance,
    requested_at: Option<Duration>,
    mapped_at: Option<Duration>,
    activity: Activity,
    save: Option<SaveJob>,
    pending_history: Option<PendingHistory>,
    history_unavailable: bool,
    saved_entry: Option<Entry>,
    retry_entry: Option<(u64, Entry)>,
    modifiers: HashMap<wl_keyboard::WlKeyboard, Modifiers>,
    loop_handle: LoopHandle<'static, App>,
    dirty: bool,
    last_paint: Duration,
}
impl App {
    fn entrance_frame(&self, now: Duration) -> crate::breathing::Frame {
        self.entrance.frame(
            self.mapped_at
                .map_or(Duration::ZERO, |at| now.saturating_sub(at)),
        )
    }
    fn reflection_input_ready(&self, now: Duration) -> bool {
        self.reflections_enabled && self.entrance.ready(self.mapped_at, now)
    }
    fn submit(&mut self, now: Duration) {
        // One in-flight submission: repeated Enter/monitor events cannot duplicate it.
        if self.save.is_some() || !self.reflection_input_ready(now) {
            return;
        }
        let Some((generation, text)) = self.activity.enter(now) else {
            return;
        };
        let candidate = if let Some(original) = &self.saved_entry {
            original.corrected(&text)
        } else if let Some((cached_generation, cached)) = &self.retry_entry
            && *cached_generation == generation
            && cached.text == text
        {
            Ok(cached.clone())
        } else {
            Entry::submitted(&text)
        };
        let entry = match candidate {
            Ok(entry) => entry,
            Err(error) => {
                self.activity.failed(generation, error);
                return;
            }
        };
        self.retry_entry = Some((generation, entry.clone()));
        let pending = if let Some(original) = &self.saved_entry {
            PendingSave::start_correction(original.clone(), entry.clone())
        } else {
            PendingSave::start(entry.clone())
        };
        match pending {
            Ok(result) => {
                self.save = Some(SaveJob {
                    generation,
                    entry,
                    result,
                })
            }
            Err(error) => self.activity.failed(generation, error),
        }
    }
    fn poll_history(&mut self) {
        let Some(result) = self.pending_history.as_ref().and_then(PendingHistory::poll) else {
            return;
        };
        self.pending_history = None;
        self.chart_revision += 1;
        match result {
            Ok(mut history) => {
                // A slow history load must retain vocabulary saved meanwhile.
                if let Some(entry) = &self.saved_entry {
                    history.vocabulary.include(entry.metadata());
                    history
                        .charts
                        .upsert(entry, recall::system_ms(), dashboard::to_local);
                }
                self.activity.recall = history;
            }
            Err(error) => {
                self.history_unavailable = true;
                eprintln!("Previous reflections unavailable: {error}");
            }
        }
        self.dirty = true;
    }
    fn poll_save(&mut self, now: Duration) {
        let Some(job) = &mut self.save else {
            return;
        };
        let Some(result) = job.result.poll() else {
            return;
        };
        let generation = job.generation;
        let entry = job.entry.clone();
        self.save = None;
        match result {
            Ok(()) => {
                self.activity.saved(generation, &entry.text, now);
                self.activity.recall.vocabulary.include(entry.metadata());
                self.activity.recall.charts.upsert(
                    &entry,
                    recall::system_ms(),
                    dashboard::to_local,
                );
                self.chart_revision += 1;
                self.saved_entry = Some(entry);
                self.retry_entry = None;
            }
            Err(error) => self.activity.failed(generation, error),
        }
        self.dirty = true;
    }
    fn key(&mut self, keyboard: &wl_keyboard::WlKeyboard, event: KeyEvent, repeated: bool) {
        let now = boot_time();
        if self.mapped_at.is_none() || self.reason.is_some() || self.activity.expired(now).is_some()
        {
            return;
        }
        if !self.reflection_input_ready(now) {
            if event.keysym == Keysym::Escape && !repeated {
                self.reason = Some("escape");
            }
            return;
        }
        if event.keysym == Keysym::Escape {
            if !repeated && !self.activity.dismiss_suggestions() && self.activity.cancel(now) {
                self.reason = Some("escape");
            }
            self.dirty = true;
            return;
        }
        if self
            .save
            .as_ref()
            .is_some_and(|job| job.generation == self.activity.generation())
        {
            return;
        }
        let modifiers = self.modifiers.get(keyboard).copied().unwrap_or_default();
        if matches!(event.keysym, Keysym::Return | Keysym::KP_Enter) {
            if !repeated {
                self.submit(now);
            }
        } else if !self.activity.field_visible() {
            return;
        } else if modifiers.ctrl && matches!(event.keysym, Keysym::a | Keysym::A) {
            self.activity.leave_history(now);
            self.activity.reset_completion();
            self.activity.editor.select_all();
        } else if !modifiers.ctrl && !modifiers.alt && !modifiers.logo {
            match event.keysym {
                Keysym::Up if !modifiers.shift => {
                    if !self.activity.move_suggestion(false) {
                        self.activity.browse_older(now);
                    }
                }
                Keysym::Down if !modifiers.shift => {
                    if !self.activity.move_suggestion(true) {
                        self.activity.browse_newer(now);
                    }
                }
                Keysym::Tab if !modifiers.shift => {
                    if !repeated {
                        self.activity.complete(now);
                    }
                }
                Keysym::BackSpace => self.activity.backspace(now),
                Keysym::Delete => self.activity.delete(now),
                Keysym::Left => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.left(modifiers.shift);
                }
                Keysym::Right => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.right(modifiers.shift);
                }
                Keysym::Home => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.home(modifiers.shift);
                }
                Keysym::End => {
                    self.activity.leave_history(now);
                    self.activity.reset_completion();
                    self.activity.editor.end(modifiers.shift);
                }
                _ => {
                    if let Some(text) = &event.utf8 {
                        self.activity.insert(text, now);
                    }
                }
            }
        }
        self.dirty = true;
    }
    fn repaint(&mut self, now: Duration) -> Result<(), Box<dyn Error>> {
        if self
            .focus_snapshot
            .as_mut()
            .is_some_and(|snapshot| snapshot.day.advance_day(recall::system_ms()))
        {
            self.chart_revision += 1;
            self.dirty = true;
        }
        if self.charts.kinds().next().is_some()
            && self
                .activity
                .recall
                .charts
                .advance(dashboard::to_local(recall::system_ms()))
        {
            self.chart_revision += 1;
            self.dirty = true;
        }
        let entrance = self.entrance_frame(now);
        if !entrance.active && self.entrance_frame(self.last_paint).active {
            self.dirty = true;
        }
        let interval = if entrance.active {
            Duration::from_millis(33)
        } else if self.activity.recall.animating(now)
            || self.activity.recall.animating(self.last_paint)
        {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        };
        if !self.dirty
            && ((!entrance.active && !self.reflections_enabled)
                || !self.activity.field_visible()
                || now.saturating_sub(self.last_paint) < interval)
        {
            return Ok(());
        }
        for index in 0..self.active.len() {
            if self.active[index].mapped {
                self.draw(index, now)?;
            }
        }
        self.last_paint = now;
        self.dirty =
            !entrance.active && self.active.iter().any(|a| a.entrance_dirty.contains(&true));
        Ok(())
    }
    fn show(&mut self, qh: &QueueHandle<Self>, outputs: &[wl_output::WlOutput]) {
        self.requested_at = Some(boot_time());
        if self.reflections_enabled {
            match PendingHistory::start() {
                Ok(pending) => self.pending_history = Some(pending),
                Err(error) => {
                    self.history_unavailable = true;
                    eprintln!("Previous reflections unavailable: {error}");
                }
            }
        }
        for output in outputs {
            let layer = self.layer_shell.create_layer_surface(
                qh,
                self.compositor.create_surface(qh),
                Layer::Overlay,
                Some("naal-focus-overlay"),
                Some(output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_size(0, 0);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            layer.commit();
            let scale = self
                .outputs
                .info(output)
                .map_or(1, |info| info.scale_factor.max(1));
            self.active.push(Active {
                layer,
                output: output.clone(),
                size: (0, 0),
                scale,
                mapped: false,
                keyboard_focus_observed: false,
                buffers: Vec::new(),
                chart_versions: Vec::new(),
                entrance_dirty: Vec::new(),
                entrance_backdrops: Vec::new(),
                reveal_cache: None,
                buffer_size: (0, 0, 0),
            });
        }
        println!(
            "Requested {} fullscreen overlays · exclusive keyboard · coordinated dismissal",
            outputs.len()
        );
    }
    fn draw(&mut self, index: usize, now: Duration) -> Result<(), Box<dyn Error>> {
        let active = &self.active[index];
        let scale = active.scale.clamp(1, 4) as u32;
        let width = active.size.0.checked_mul(scale).ok_or("Width overflow")?;
        let height = active.size.1.checked_mul(scale).ok_or("Height overflow")?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32_000_000 {
            return Err(format!("Unsupported size {width}x{height}").into());
        }
        let total: u64 = self
            .active
            .iter()
            .map(|a| {
                u64::from(a.size.0) * u64::from(a.size.1) * (a.scale.clamp(1, 4) as u64).pow(2)
            })
            .sum();
        // Two reusable buffers plus a temporary reveal cache per output. Keep
        // the same 512 MB aggregate pixel-storage ceiling as the old two-buffer path.
        let copies = if self.entrance.settings.enabled { 3 } else { 2 };
        if total.saturating_mul(copies) > 128_000_000 {
            return Err("Combined monitor buffers exceed the safety budget.".into());
        }
        if self.active[index].buffer_size != (width, height, scale) {
            self.active[index].buffers.clear();
            self.active[index].chart_versions.clear();
            self.active[index].entrance_dirty.clear();
            self.active[index].entrance_backdrops.clear();
            self.active[index].reveal_cache = None;
            self.active[index].buffer_size = (width, height, scale);
        }
        let entrance = self.entrance_frame(now);
        let active = &self.active[index];
        let available = active
            .buffers
            .iter()
            .enumerate()
            .filter(|(_, buffer)| self.pool.canvas(*buffer).is_some())
            // Restore both caches before returning to ordinary input updates.
            .min_by_key(|(i, _)| entrance.active || !active.entrance_dirty[*i])
            .map(|(i, _)| i);
        let (buffer_index, fresh) = if let Some(index) = available {
            (index, false)
        } else if self.active[index].buffers.len() < 2 {
            let (buffer, _) = self.pool.create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            self.active[index].buffers.push(buffer);
            self.active[index].chart_versions.push(0);
            self.active[index].entrance_dirty.push(false);
            self.active[index].entrance_backdrops.push([0; 4]);
            (self.active[index].buffers.len() - 1, true)
        } else {
            // Never allocate an unbounded queue of frames if the compositor is
            // slow or stops releasing buffers during a long writing session.
            return Ok(());
        };
        let restore = self.active[index].entrance_dirty[buffer_index];
        let full = fresh || restore;
        let charts_changed =
            full || self.active[index].chart_versions[buffer_index] != self.chart_revision;
        let background = render::breathing_background(entrance);
        let full_entrance =
            fresh || self.active[index].entrance_backdrops[buffer_index] != background;
        if !entrance.active {
            self.active[index].reveal_cache = None;
        }
        let content = render::focus_content(
            width as f32,
            height as f32,
            scale as f32,
            &self.focus_settings,
        );
        let mut reveal_cache = None;
        let buffer = &self.active[index].buffers[buffer_index];
        let canvas = self.pool.canvas(buffer).ok_or("Buffer is still in use")?;
        if entrance.active && entrance.reflection == 0.0 {
            render::render_breathing(
                canvas,
                (width as usize, height as usize),
                self.font.as_ref().ok_or("Font not loaded")?,
                scale as f32,
                render::BreathingView {
                    pattern: &self.entrance.pattern,
                    frame: entrance,
                    full: full_entrance,
                },
            );
        } else if entrance.active && self.active[index].reveal_cache.is_some() {
            render::render_reveal(
                canvas,
                self.active[index].reveal_cache.as_ref().unwrap(),
                entrance.reflection,
            );
        } else {
            if full {
                render::render_overlay(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    render::OverlayView {
                        quote: &self.quote,
                        text_size: self.text_size as f32 * scale as f32,
                        reflections_enabled: self.reflections_enabled,
                        charts: &self.charts,
                        focus: &self.focus_settings,
                    },
                );
            }
            if charts_changed {
                render::render_charts_in(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    content,
                    render::ChartView {
                        settings: &self.charts,
                        data: self
                            .pending_history
                            .is_none()
                            .then_some(&self.activity.recall.charts),
                        unavailable: self.history_unavailable,
                    },
                );
                render::render_focus(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    &self.focus_settings,
                    self.focus_snapshot.as_ref(),
                );
            }
            if self.reflections_enabled {
                let saving = self
                    .save
                    .as_ref()
                    .is_some_and(|job| job.generation == self.activity.generation());
                render::render_input_in(
                    canvas,
                    (width as usize, height as usize),
                    self.font.as_ref().ok_or("Font not loaded")?,
                    scale as f32,
                    content,
                    render::InputView {
                        editor: &self.activity.editor,
                        suggestions: &self.activity.suggestions(),
                        suggestion_index: self.activity.suggestion_index(),
                        saved_text: self.activity.displayed_text(),
                        progress: self.activity.typing_fraction(now),
                        message: if saving {
                            Some("Saving…")
                        } else if let Some(error) = self.activity.error.as_deref() {
                            Some(error)
                        } else if self.history_unavailable && !self.activity.editing() {
                            Some("Previous reflections unavailable · type to reflect")
                        } else {
                            None
                        },
                        editing: self.activity.editing(),
                        caret_visible: self.activity.recall.selected().is_none()
                            && (now.as_millis() / 500).is_multiple_of(2),
                        history: Some(render::HistoryView {
                            recall: &self.activity.recall,
                            now,
                            now_ms: recall::system_ms(),
                        }),
                    },
                );
            }
            if entrance.active {
                let cache = canvas.to_vec();
                render::render_reveal(canvas, &cache, entrance.reflection);
                reveal_cache = Some(cache);
            }
        }
        // Seed the second buffer now, not on the user's first keystroke. This
        // avoids another full-screen/font rasterization in the typing path.
        let seed = if full && !entrance.active && self.active[index].buffers.len() == 1 {
            Some(canvas.to_vec())
        } else {
            None
        };
        let active = &self.active[index];
        let surface = active.layer.wl_surface();
        surface.set_buffer_scale(scale as i32);
        if (entrance.active && (full_entrance || entrance.reflection > 0.0))
            || (!entrance.active && full)
        {
            surface.damage_buffer(0, 0, width as i32, height as i32);
        } else if entrance.active {
            let damage = render::breathing_damage(width as f32, height as f32, scale as f32);
            surface.damage_buffer(
                damage.x.floor() as i32,
                damage.y.floor() as i32,
                damage.width.ceil() as i32 + 1,
                damage.height.ceil() as i32 + 1,
            );
        } else {
            let damage = render::input_damage(content.width, content.height, scale as f32);
            surface.damage_buffer(
                content.x.floor() as i32,
                damage.y.floor() as i32,
                content.width.ceil() as i32,
                damage.height.ceil() as i32,
            );
            if charts_changed
                && let Some(band) = render::chart_damage_in(content, scale as f32, &self.charts)
            {
                surface.damage_buffer(
                    band.x.floor() as i32,
                    band.y.floor() as i32,
                    band.width.ceil() as i32 + 1,
                    band.height.ceil() as i32 + 1,
                );
            }
        }
        if !entrance.active
            && charts_changed
            && !full
            && let Some(sidebar) = render::focus_sidebar(
                width as f32,
                height as f32,
                scale as f32,
                &self.focus_settings,
            )
        {
            surface.damage_buffer(
                sidebar.x.floor() as i32,
                sidebar.y.floor() as i32,
                sidebar.width.ceil() as i32,
                sidebar.height.ceil() as i32,
            );
        }
        buffer.attach_to(surface)?;
        active.layer.commit();
        let active = &mut self.active[index];
        active.chart_versions[buffer_index] = if entrance.active {
            0
        } else {
            self.chart_revision
        };
        if let Some(cache) = reveal_cache {
            active.reveal_cache = Some(cache);
        }
        active.entrance_dirty[buffer_index] = entrance.active;
        active.entrance_backdrops[buffer_index] = background;
        if !active.mapped {
            active.mapped = true;
            if self.mapped_at.is_none() {
                let mapped_now = boot_time();
                self.mapped_at = Some(mapped_now);
                // The normal countdown starts only after the complete entrance;
                // early keyboard/pointer events are separately gated above.
                self.activity
                    .mapped(mapped_now + self.entrance.settings.duration());
            }
            let name = self
                .outputs
                .info(&active.output)
                .and_then(|i| i.name)
                .unwrap_or_else(|| "(unnamed)".into());
            println!("Buffer committed: {name} · {width}x{height} · scale {scale}");
        }
        if let Some(seed) = seed {
            let (buffer, canvas) = self.pool.create_buffer(
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            canvas.copy_from_slice(&seed);
            self.active[index].buffers.push(buffer);
            self.active[index].chart_versions.push(if entrance.active {
                0
            } else {
                self.chart_revision
            });
            self.active[index].entrance_dirty.push(entrance.active);
            self.active[index].entrance_backdrops.push(background);
        }
        Ok(())
    }
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if self.active.iter().any(|a| &a.layer == layer) {
            self.reason = Some("compositor-closed");
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(index) = self.active.iter().position(|a| &a.layer == layer) {
            self.active[index].size = config.new_size;
            if let Err(e) = self.draw(index, boot_time()) {
                self.failure = Some(e.to_string());
            }
        }
    }
}
impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        scale: i32,
    ) {
        if let Some(index) = self
            .active
            .iter()
            .position(|a| a.layer.wl_surface() == surface)
        {
            self.active[index].scale = scale;
            if self.active[index].mapped
                && let Err(e) = self.draw(index, boot_time())
            {
                self.failure = Some(e.to_string());
            }
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if self.requested_at.is_some() {
            self.reason = Some("output-added");
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.reason = Some("output-removed");
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if let Some(k) = self.keyboards.remove(&seat) {
            k.release();
        }
        if let Some(p) = self.pointers.remove(&seat) {
            p.release();
        }
        if self.requested_at.is_some() {
            self.reason = Some("seat-removed");
        }
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if !self.keyboards.contains_key(&seat) => {
                match self.seats.get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    self.loop_handle.clone(),
                    Box::new(|app, keyboard, event| app.key(keyboard, event, true)),
                ) {
                    Ok(k) => {
                        self.keyboards.insert(seat, k);
                    }
                    Err(e) => self.failure = Some(format!("Keyboard setup: {e}")),
                }
            }
            Capability::Pointer if !self.pointers.contains_key(&seat) => {
                match self.seats.get_pointer(qh, &seat) {
                    Ok(p) => {
                        self.pointers.insert(seat, p);
                    }
                    Err(e) => self.failure = Some(format!("Pointer setup: {e}")),
                }
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboards.remove(&seat)
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointers.remove(&seat)
        {
            p.release();
        }
    }
}
impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if let Some(active) = self
            .active
            .iter_mut()
            .find(|a| a.layer.wl_surface() == surface)
        {
            active.keyboard_focus_observed = true;
            println!("Keyboard focus acquired by overlay");
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(keyboard, event, false);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(keyboard, event, true);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers.insert(keyboard.clone(), modifiers);
    }
}
impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        if !self.reflection_input_ready(boot_time()) {
            return;
        }
        for event in events {
            let Some(active) = self
                .active
                .iter()
                .find(|a| a.layer.wl_surface() == &event.surface)
            else {
                continue;
            };
            let scale = active.scale.clamp(1, 4) as f32;
            let content = render::focus_content(
                active.size.0 as f32 * scale,
                active.size.1 as f32 * scale,
                scale,
                &self.focus_settings,
            );
            self.dirty |= pointer_input(
                &mut self.activity,
                self.font.as_ref(),
                &event.kind,
                ((content.width / scale).round() as u32, active.size.1),
                scale,
                (
                    event.position.0 - f64::from(content.x / scale),
                    event.position.1,
                ),
                boot_time(),
            );
        }
    }
}
/// Pointer input only edits the reflection field; it never cancels or dismisses.
fn pointer_input(
    activity: &mut Activity,
    font: Option<&Font>,
    kind: &PointerEventKind,
    size: (u32, u32),
    scale: f32,
    position: (f64, f64),
    now: Duration,
) -> bool {
    if !matches!(kind, PointerEventKind::Press { .. }) {
        return false;
    }
    let field = render::input_rect(size.0 as f32 * scale, size.1 as f32 * scale, scale);
    if !field.contains(position.0 as f32 * scale, position.1 as f32 * scale) {
        return false;
    }
    let mut changed = activity.leave_history(now);
    changed |= activity.open_saved(now);
    if let Some(caret) =
        font.and_then(|font| render::input_caret(font, &activity.editor, size, scale, position))
    {
        activity.reset_completion();
        activity.editor.set_cursor(caret, false);
        changed = true;
    }
    changed
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
delegate_registry!(App);
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(App);

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(args.iter().map(|s| (*s).into()))
    }
    fn press() -> PointerEventKind {
        PointerEventKind::Press {
            time: 0,
            button: 0x110,
            serial: 0,
        }
    }
    fn release() -> PointerEventKind {
        PointerEventKind::Release {
            time: 0,
            button: 0x110,
            serial: 0,
        }
    }

    #[test]
    fn background_clicks_and_field_releases_preserve_reflection_and_timing() {
        let now = Duration::from_secs(2);
        for state in 0..5 {
            let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
            activity.mapped(Duration::ZERO);
            if state > 0 {
                activity.insert("Reflection", Duration::from_secs(1));
            }
            if state > 1 {
                let (generation, text) = activity.submission().unwrap();
                activity.saved(generation, &text, now);
            }
            if state > 2 {
                activity.open_saved(now);
            }
            if state > 3 {
                activity.insert(" correction", now);
            }
            let text = activity.editor.text().to_owned();
            let displayed = activity.displayed_text().map(str::to_owned);
            let editing = activity.editing();
            let visible = activity.field_visible();
            let generation = activity.generation();
            let remaining = activity.remaining(Duration::from_secs(3));
            let typing = activity.typing_fraction(Duration::from_secs(3));
            let field = render::input_rect(1920.0, 1080.0, 1.0);
            for (kind, position) in [
                (press(), (0.0, 0.0)),
                (release(), (0.0, 0.0)),
                // Releasing inside the field must also be a no-op.
                (release(), (field.x as f64 + 1.0, field.y as f64 + 1.0)),
            ] {
                assert!(!pointer_input(
                    &mut activity,
                    None,
                    &kind,
                    (1920, 1080),
                    1.0,
                    position,
                    now
                ));
                assert_eq!(activity.editor.text(), text);
                assert_eq!(activity.displayed_text(), displayed.as_deref());
                assert_eq!(activity.editing(), editing);
                assert_eq!(activity.field_visible(), visible);
                assert_eq!(activity.generation(), generation);
                assert_eq!(activity.remaining(Duration::from_secs(3)), remaining);
                assert_eq!(activity.typing_fraction(Duration::from_secs(3)), typing);
            }
        }
    }

    #[test]
    fn history_rows_are_keyboard_only_and_field_click_restores_the_draft() {
        let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
        activity.mapped(Duration::ZERO);
        activity.recall.groups.push(crate::recall::Group {
            text: "Previous".into(),
            timestamp_unix_ms: 0,
            count: 1,
        });
        activity.insert("My draft", Duration::from_secs(1));
        activity.browse_older(Duration::from_secs(2));
        let field = render::input_rect(1920.0, 1080.0, 1.0);
        let now = Duration::from_secs(3);
        assert!(!pointer_input(
            &mut activity,
            None,
            &press(),
            (1920, 1080),
            1.0,
            (field.x as f64 + 20.0, field.y as f64 - 30.0),
            now
        ));
        assert_eq!(activity.recall.selected(), Some(0));
        assert_eq!(activity.editor.text(), "My draft");
        assert_eq!(activity.remaining(now), Duration::from_secs(9));
        assert!(pointer_input(
            &mut activity,
            None,
            &press(),
            (1920, 1080),
            1.0,
            (field.x as f64 + 20.0, field.y as f64 + 10.0),
            now
        ));
        assert_eq!(activity.recall.selected(), None);
        assert_eq!(activity.editor.text(), "My draft");
        assert!(activity.editing());
        assert_eq!(activity.remaining(now), Duration::from_secs(9));
    }
    #[test]
    fn field_press_still_reopens_saved_text_and_positions_caret_without_pausing() {
        let font = render::default_font().unwrap();
        let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
        activity.mapped(Duration::ZERO);
        activity.insert("Reflection", Duration::from_secs(1));
        let (generation, text) = activity.submission().unwrap();
        activity.saved(generation, &text, Duration::from_secs(2));
        let now = Duration::from_secs(3);
        let remaining = activity.remaining(now);
        let field = render::input_rect(1920.0, 1080.0, 1.0);
        assert!(pointer_input(
            &mut activity,
            Some(&font),
            &press(),
            (1920, 1080),
            1.0,
            (field.x as f64 + 1.0, field.y as f64 + 1.0),
            now,
        ));
        assert!(activity.field_visible());
        assert!(!activity.editing());
        assert_eq!(activity.editor.text(), "Reflection");
        assert_eq!(activity.editor.cursor(), 0);
        assert_eq!(activity.remaining(now), remaining);
        // A drag/release outside the field cannot cancel the reopened editor.
        assert!(!pointer_input(
            &mut activity,
            None,
            &release(),
            (1920, 1080),
            1.0,
            (0.0, 0.0),
            now
        ));
        assert!(activity.field_visible());
    }

    #[test]
    fn helper_reads_multiline_text_as_one_raw_quote_without_surfaces() {
        let quote = "Take a breath.\n\n- Notice.\n- Return.";
        assert_eq!(read_stdin_quote(quote.as_bytes()).unwrap(), quote);
        for bad in [
            String::new(),
            "a".repeat(501),
            "a".repeat(4097),
            "bad\u{1b}text".into(),
        ] {
            assert!(read_stdin_quote(bad.as_bytes()).is_err());
        }
        assert!(read_stdin_quote(&[0xff][..]).is_err());
    }

    #[test]
    fn sidebars_preserve_scaled_field_hit_testing_and_background_clicks() {
        let font = Font::from_bytes(
            std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf").unwrap(),
            fontdue::FontSettings::default(),
        )
        .unwrap();
        for side in [FocusSide::Left, FocusSide::Right] {
            for scale in [1.0, 2.0] {
                let settings = crate::config::FocusSettings {
                    enabled: true,
                    side,
                    ..Default::default()
                };
                let content =
                    render::focus_content(1920.0 * scale, 1080.0 * scale, scale, &settings);
                let field = render::input_rect(content.width, content.height, scale);
                let virtual_size = ((content.width / scale) as u32, 1080);
                let mut activity = Activity::new(Duration::from_secs(10), Duration::from_secs(30));
                activity.mapped(Duration::ZERO);
                activity.insert("Saved reflection", Duration::from_secs(1));
                let (generation, text) = activity.submission().unwrap();
                activity.saved(generation, &text, Duration::from_secs(2));
                // Same coordinate transform used by the live pointer handler.
                let global = (
                    (content.x + field.x + 15.0 * scale) / scale,
                    (field.y + 10.0 * scale) / scale,
                );
                let local = (f64::from(global.0 - content.x / scale), f64::from(global.1));
                assert!(pointer_input(
                    &mut activity,
                    Some(&font),
                    &press(),
                    virtual_size,
                    scale,
                    local,
                    Duration::from_secs(3)
                ));
                assert_eq!(activity.editor.cursor(), 0);
                let sidebar =
                    render::focus_sidebar(1920.0 * scale, 1080.0 * scale, scale, &settings)
                        .unwrap();
                let background = (
                    f64::from((sidebar.x - content.x + 10.0 * scale) / scale),
                    f64::from((sidebar.y + 10.0 * scale) / scale),
                );
                assert!(!pointer_input(
                    &mut activity,
                    Some(&font),
                    &press(),
                    virtual_size,
                    scale,
                    background,
                    Duration::from_secs(3)
                ));
            }
        }
    }

    #[test]
    fn focus_cli_and_private_payload_are_validated_before_capturing_focus() {
        for args in [
            vec!["--focus-style"],
            vec!["--focus-style", "pie"],
            vec!["--focus-side", "top"],
            vec!["--stdin", "--payload"],
        ] {
            assert!(parse(&args).is_err());
        }
        let selected = parse(&[
            "--focus-history",
            "--focus-style",
            "timeline",
            "--focus-side",
            "left",
            "--focus-24-hour",
        ])
        .unwrap()
        .unwrap();
        assert!(selected.settings.focus.enabled);
        assert_eq!(selected.settings.focus.style, FocusStyle::Timeline);
        assert_eq!(selected.settings.focus.side, FocusSide::Left);
        assert!(selected.settings.focus.use_24_hour_time);
        let clock_only = parse(&["--focus-24-hour"]).unwrap().unwrap();
        assert!(clock_only.settings.focus.use_24_hour_time);
        assert!(!clock_only.settings.focus.enabled);
        assert!(!parse(&[]).unwrap().unwrap().settings.focus.use_24_hour_time);
        let payload = Payload {
            quote: "Take a breath.\nReturn.".into(),
            focus: Some(focus::Snapshot {
                day: focus::Day::empty(recall::system_ms()),
                unavailable: false,
            }),
        };
        let bytes = serde_json::to_vec(&payload).unwrap();
        assert_eq!(read_payload(bytes.as_slice()).unwrap().quote, payload.quote);
        for invalid in [
            b"{}".as_slice(),
            b"not json",
            br#"{"quote":"", "focus":null}"#,
            br#"{"quote":"ok", "focus":null,"extra":1}"#,
        ] {
            assert!(read_payload(invalid).is_err());
        }
    }

    #[test]
    fn chart_cli_requires_known_styles_and_categories() {
        for args in [
            vec!["--chart-style"],
            vec!["--chart-style", "pie"],
            vec!["--charts"],
            vec!["--charts", ""],
            vec!["--charts", "tags,unknown"],
            vec!["--charts", "none,tags"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        let selected = parse(&["--charts", "projects,tags", "--chart-style", "treemap"])
            .unwrap()
            .unwrap();
        assert_eq!(
            selected.settings.charts,
            ChartSettings {
                style: ChartStyle::Treemap,
                projects: true,
                feelings: false,
                tags: true
            }
        );
        assert_eq!(
            parse(&["--charts", "tags", "--charts", "none"])
                .unwrap()
                .unwrap()
                .settings
                .charts
                .kinds()
                .count(),
            0
        );
    }

    #[test]
    fn reflections_cli_defaults_on_and_off_suppresses_charts_not_focus() {
        assert!(parse(&[]).unwrap().unwrap().settings.reflections_enabled);
        for (mode, enabled) in [("on", true), ("off", false)] {
            let settings = parse(&[
                "--reflections",
                mode,
                "--charts",
                "projects,feelings,tags",
                "--focus-history",
            ])
            .unwrap()
            .unwrap()
            .settings;
            assert_eq!(settings.reflections_enabled, enabled);
            assert_eq!(settings.charts.kinds().count(), 3);
            assert_eq!(
                settings.overlay_charts().kinds().count(),
                if enabled { 3 } else { 0 }
            );
            assert!(settings.focus.enabled);
            assert!(settings.breathing.enabled);
        }
        for args in [
            vec!["--reflections"],
            vec!["--reflections", "yes"],
            vec!["--reflections", "false"],
        ] {
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn breathing_cli_rejects_unbounded_or_unknown_preferences() {
        for args in [
            vec!["--breathing", "yes"],
            vec!["--breath-pace", "fast"],
            vec!["--breath-cycles", "0"],
            vec!["--breath-cycles", "3"],
            vec!["--breath-cycles", "256"],
            vec!["--breath-patterns", ""],
            vec!["--breath-patterns", "lattice,lattice"],
            vec!["--breath-patterns", "random"],
            vec!["--breath-patterns"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
        let options = Options::parse(["--breathing", "off", "--reduced-motion"].map(str::to_owned))
            .unwrap()
            .unwrap();
        assert!(!options.settings.breathing.enabled);
        assert!(options.settings.breathing.reduced_motion);
    }

    #[test]
    fn breathing_preserves_the_entire_reflection_and_typing_budgets() {
        use crate::breathing::{Pace, Settings as BreathingSettings};
        for enabled in [false, true] {
            for pace in Pace::ALL {
                for cycles in [1, 2] {
                    let settings = BreathingSettings {
                        enabled,
                        pace,
                        cycles,
                        ..BreathingSettings::default()
                    };
                    let entrance = Entrance::new(settings.clone(), 42);
                    let mapped = Duration::from_secs(10);
                    let ready = mapped + settings.duration();
                    let mut activity =
                        Activity::new(Duration::from_secs(5), Duration::from_secs(30));
                    activity.mapped(ready);
                    assert!(!entrance.ready(None, ready));
                    assert!(!entrance.ready(Some(mapped), ready - Duration::from_millis(1)));
                    assert!(entrance.ready(Some(mapped), ready));
                    assert_eq!(activity.expired(mapped), None);
                    assert_eq!(activity.remaining(mapped), Duration::from_secs(5));
                    assert_eq!(activity.remaining(ready), Duration::from_secs(5));
                    assert_eq!(activity.expired(ready + Duration::from_secs(4)), None);
                    activity.insert("Reflect", ready + Duration::from_secs(1));
                    assert_eq!(
                        activity.remaining(ready + Duration::from_secs(30)),
                        Duration::from_secs(4)
                    );
                    assert_eq!(activity.expired(ready + Duration::from_secs(30)), None);
                    assert_eq!(
                        activity.expired(ready + Duration::from_secs(31)),
                        Some(Expiry::Typing)
                    );
                }
            }
        }
    }

    #[test]
    fn helper_has_no_recurring_mode_and_bounds_focus_capture() {
        assert!(parse(&[]).unwrap().unwrap().output.is_none());
        for args in [
            vec!["--loop"],
            vec!["--duration-secs", "0"],
            vec!["--duration-secs", "31"],
            vec!["--typing-limit-secs", "29"],
            vec!["--typing-limit-secs", "601"],
            vec!["--delay-secs", "61"],
            vec!["--all", "--output", "DP-1"],
            vec!["--output"],
            vec!["--text-size", "21"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert!(parse(&["--check", "--output", "DP-1"]).is_ok());
    }
}
