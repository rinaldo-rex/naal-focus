//! One coordinated, short-lived native overlay on every selected output.
mod cli;
mod drawing;
mod interaction;
mod payload;
mod wayland;

pub use cli::{HELP, Options};
pub(crate) use payload::Payload;

use crate::{
    activity::{Activity, Expiry},
    activity_log::{Entry, PendingSave},
    breathing::Entrance,
    config::ChartSettings,
    focus,
    guard::Guard,
    process::{Singleton, boot_time, desktop_connection},
    recall::PendingHistory,
    render,
};
use fontdue::Font;
use payload::{read_payload, read_stdin_quote};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::{
        calloop::{EventLoop, LoopHandle},
        calloop_wayland_source::WaylandSource,
    },
    registry::RegistryState,
    seat::{SeatState, keyboard::Modifiers},
    shell::wlr_layer::{LayerShell, LayerSurface},
    shm::{
        Shm,
        slot::{Buffer, SlotPool},
    },
};
use std::{collections::HashMap, error::Error, io::Read, time::Duration};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat},
};

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
}

#[cfg(test)]
mod tests;
