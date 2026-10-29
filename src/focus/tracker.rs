//! One supervisor-owned tracker. No screenshots, input hooks, or subprocess polling.
use super::{Clock, Day, Snapshot, Window, store};
use crate::{
    activity_log,
    process::{Singleton, boot_time, desktop_connection},
    time::system_ms,
};
use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1 as handle, zcosmic_toplevel_info_v1 as info,
};
use smithay_client_toolkit::reexports::{
    calloop::EventLoop, calloop_wayland_source::WaylandSource,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1 as foreign_handle, ext_foreign_toplevel_list_v1 as foreign_list,
};

#[derive(Clone, Copy, Default)]
struct Control {
    enabled: bool,
    allowed: bool,
    seen: Duration,
}
impl Control {
    fn fresh(self, now: Duration) -> bool {
        now >= self.seen && now.saturating_sub(self.seen) <= Duration::from_secs(3)
    }
}
struct Shared {
    day: Day,
    ready: bool,
    error: Option<String>,
    storage_error: Option<String>,
}

pub struct Tracker {
    control: Arc<Mutex<Control>>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
}
impl Tracker {
    pub fn start() -> Self {
        let tracker = Self {
            control: Arc::new(Mutex::new(Control::default())),
            shared: Arc::new(Mutex::new(Shared {
                day: Day::empty(system_ms()),
                ready: false,
                error: None,
                storage_error: None,
            })),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let (control, shared, stop) = (
            tracker.control.clone(),
            tracker.shared.clone(),
            tracker.stop.clone(),
        );
        if std::thread::Builder::new()
            .name("focus-tracker".into())
            .spawn(move || {
                if let Err(error) = run(&control, &shared, &stop) {
                    shared.lock().unwrap().error =
                        Some(format!("Focus tracking unavailable: {error}"));
                }
            })
            .is_err()
        {
            tracker.shared.lock().unwrap().error = Some("Cannot start focus tracker.".into());
        }
        tracker
    }
    /// Reminders and snooze do not control enabled. Only unsafe sessions and a
    /// live helper suppress collection, with a heartbeat that fails closed.
    pub fn update(&self, enabled: bool, allowed: bool) {
        *self.control.lock().unwrap() = Control {
            enabled,
            allowed,
            seen: boot_time(),
        };
    }
    pub fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock().unwrap();
        let mut day = state.day.clone();
        day.advance_day(system_ms());
        Snapshot {
            day,
            unavailable: !state.ready || state.error.is_some() || state.storage_error.is_some(),
        }
    }
    pub fn error(&self, enabled: bool) -> Option<String> {
        let state = self.shared.lock().unwrap();
        // Retention failures matter even with collection switched off.
        state
            .storage_error
            .as_ref()
            .or_else(|| enabled.then_some(state.error.as_ref()).flatten())
            .cloned()
    }
}
impl Default for Tracker {
    fn default() -> Self {
        Self::start()
    }
}
impl Drop for Tracker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.update(false, false);
    }
}

fn run(control: &Mutex<Control>, shared: &Mutex<Shared>, stop: &AtomicBool) -> Result<(), String> {
    // A departing supervisor may still be flushing its worker. Do not let a
    // replacement race its final snapshot, or run one tracker per panel.
    let _singleton = loop {
        match Singleton::acquire("focus-tracker") {
            Ok(lock) => {
                shared.lock().unwrap().error = None;
                break lock;
            }
            Err(error) if !stop.load(Ordering::Relaxed) => {
                shared.lock().unwrap().error =
                    Some(format!("Waiting for shared focus tracker: {error}"));
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Ok(()),
        }
    };
    let directory: PathBuf = activity_log::directory()?;
    let loaded = loop {
        match store::load_today_in(&directory, system_ms()) {
            Ok(day) => break day,
            Err(error) => {
                shared.lock().unwrap().storage_error =
                    Some(format!("Focus history unavailable: {error}"));
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    };
    {
        let mut state = shared.lock().unwrap();
        state.day = loaded;
        state.storage_error = None;
    }
    let mut monitor: Option<Monitor> = None;
    let mut clock = Clock::default();
    let mut dirty = false;
    let mut persisted = directory.join("focus.json").exists();
    let mut last_save = boot_time();
    let mut retry_at = Duration::ZERO;
    let mut was_allowed = false;
    let mut full_day: Option<String> = None;
    loop {
        let now = boot_time();
        let command = *control.lock().unwrap();
        let enabled = command.enabled && command.fresh(now) && !stop.load(Ordering::Relaxed);
        if !enabled {
            monitor = None;
        }
        if enabled && monitor.is_none() && now >= retry_at {
            match Monitor::connect() {
                Ok(connection) => {
                    monitor = Some(connection);
                    shared.lock().unwrap().error = None;
                }
                Err(error) => {
                    shared.lock().unwrap().error =
                        Some(format!("Focus tracking unavailable: {error}"));
                    retry_at = now + Duration::from_secs(5);
                }
            }
        }
        if let Some(connection) = &mut monitor {
            if let Err(error) = connection.dispatch() {
                shared.lock().unwrap().error = Some(format!("Focus tracking unavailable: {error}"));
                monitor = None;
                retry_at = boot_time() + Duration::from_secs(5);
            }
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
        let now = boot_time();
        let wall = system_ms();
        let command = *control.lock().unwrap();
        let allowed = enabled
            && command.enabled
            && command.allowed
            && command.fresh(now)
            && !stop.load(Ordering::Relaxed)
            && monitor.as_ref().is_some_and(|m| m.state.ready);
        let window = monitor.as_ref().and_then(|m| m.state.current.clone());
        let (rolled, day) = {
            let mut state = shared.lock().unwrap();
            let previous_date = state.day.date.clone();
            // Once full, resume only on the next local day.
            let not_full = full_day.as_ref() != Some(&state.day.date);
            match clock.observe(&mut state.day, now, wall, window, allowed && not_full) {
                Ok(changed) => dirty |= changed,
                Err(error) => {
                    full_day = Some(state.day.date.clone());
                    state.error = Some(error);
                }
            }
            let rolled = previous_date != state.day.date;
            if rolled {
                full_day = None;
                state.error = None;
            }
            state.ready = !enabled || monitor.as_ref().is_some_and(|m| m.state.ready);
            let flush = dirty
                && (persisted || !state.day.sessions.is_empty())
                && (rolled
                    || was_allowed && !allowed
                    || stop.load(Ordering::Relaxed)
                    || now.saturating_sub(last_save) >= Duration::from_secs(15));
            (rolled, flush.then(|| state.day.clone()))
        };
        if let Some(day) = day {
            // No supervisor, rendering, input, or safety mutex is held during I/O.
            match store::save_in(&directory, &day, wall) {
                Ok(()) => {
                    dirty = false;
                    persisted = true;
                    shared.lock().unwrap().storage_error = None;
                }
                Err(error) => {
                    shared.lock().unwrap().storage_error =
                        Some(format!("Focus history could not be saved: {error}"))
                }
            }
            last_save = now;
        } else if rolled {
            // Empty/disabled trackers still purge yesterday without creating
            // history on a first install.
            if let Err(error) = store::load_today_in(&directory, wall) {
                shared.lock().unwrap().storage_error = Some(error);
            }
        }
        was_allowed = allowed;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
    }
}

struct Toplevel {
    foreign: foreign_handle::ExtForeignToplevelHandleV1,
    cosmic: handle::ZcosmicToplevelHandleV1,
    window: Window,
    active: bool,
    has_state: bool,
}
struct State {
    info: info::ZcosmicToplevelInfoV1,
    windows: Vec<Toplevel>,
    current: Option<Window>,
    ready: bool,
    failure: Option<String>,
    globals: Vec<u32>,
}
impl State {
    fn commit(&mut self) {
        // Multiple activated toplevels are ambiguous, not additive work. This
        // also avoids counting windows on two monitors simultaneously.
        let mut active = self.windows.iter().filter(|w| w.active && w.has_state);
        self.current = active.next().map(|w| w.window.clone());
        if active.next().is_some() {
            self.current = None;
        }
        self.ready = true;
    }
}
struct Monitor {
    event_loop: EventLoop<'static, State>,
    state: State,
}
impl Monitor {
    fn connect() -> Result<Self, String> {
        let conn = desktop_connection()?;
        let (globals, queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let info: info::ZcosmicToplevelInfoV1 = globals
            .bind(&qh, 2..=2, ())
            .map_err(|_| "COSMIC window-info protocol is unavailable.")?;
        let _list: foreign_list::ExtForeignToplevelListV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "Desktop window-list protocol is unavailable.")?;
        let names = globals
            .contents()
            .clone_list()
            .into_iter()
            .filter(|g| {
                g.interface == "zcosmic_toplevel_info_v1"
                    || g.interface == "ext_foreign_toplevel_list_v1"
            })
            .map(|g| g.name)
            .collect();
        let state = State {
            info,
            windows: Vec::new(),
            current: None,
            ready: false,
            failure: None,
            globals: names,
        };
        let event_loop = EventLoop::try_new().map_err(|e| e.to_string())?;
        WaylandSource::new(conn, queue)
            .insert(event_loop.handle())
            .map_err(|e| e.to_string())?;
        Ok(Self { event_loop, state })
    }
    fn dispatch(&mut self) -> Result<(), String> {
        self.event_loop
            .dispatch(Duration::from_millis(100), &mut self.state)
            .map_err(|e| e.to_string())?;
        if let Some(error) = &self.state.failure {
            return Err(error.clone());
        }
        Ok(())
    }
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        app: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event,wl_registry::Event::GlobalRemove {name} if app.globals.contains(&name)) {
            app.failure = Some("Desktop window-info protocol disappeared.".into());
            app.current = None;
            app.ready = false;
        }
    }
}
impl Dispatch<foreign_list::ExtForeignToplevelListV1, ()> for State {
    fn event(
        app: &mut Self,
        _: &foreign_list::ExtForeignToplevelListV1,
        event: foreign_list::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            foreign_list::Event::Toplevel { toplevel } => {
                if app.windows.len() >= 4096 {
                    toplevel.destroy();
                    app.failure = Some("Too many desktop windows.".into());
                    return;
                }
                let cosmic = app.info.get_cosmic_toplevel(&toplevel, qh, ());
                app.windows.push(Toplevel {
                    foreign: toplevel,
                    cosmic,
                    window: Window::new("", ""),
                    active: false,
                    has_state: false,
                });
            }
            foreign_list::Event::Finished => {
                app.failure = Some("Desktop window list stopped.".into());
                app.ready = false;
                app.current = None;
            }
            _ => {}
        }
    }
    event_created_child!(State,foreign_list::ExtForeignToplevelListV1,[foreign_list::EVT_TOPLEVEL_OPCODE=>(foreign_handle::ExtForeignToplevelHandleV1,())]);
}
impl Dispatch<foreign_handle::ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        app: &mut Self,
        proxy: &foreign_handle::ExtForeignToplevelHandleV1,
        event: foreign_handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(index) = app.windows.iter().position(|w| &w.foreign == proxy) else {
            return;
        };
        match event {
            foreign_handle::Event::AppId { app_id } => {
                let w = &mut app.windows[index];
                w.window = Window::new(&app_id, &w.window.title);
            }
            foreign_handle::Event::Title { title } => {
                let w = &mut app.windows[index];
                w.window = Window::new(&w.window.app_id, &title);
            }
            foreign_handle::Event::Done => app.commit(),
            foreign_handle::Event::Closed => {
                let w = app.windows.remove(index);
                w.cosmic.destroy();
                proxy.destroy();
                app.commit();
            }
            _ => {}
        }
    }
}
impl Dispatch<info::ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        app: &mut Self,
        _: &info::ZcosmicToplevelInfoV1,
        event: info::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, info::Event::Done) {
            app.commit();
        }
    }
}
impl Dispatch<handle::ZcosmicToplevelHandleV1, ()> for State {
    fn event(
        app: &mut Self,
        proxy: &handle::ZcosmicToplevelHandleV1,
        event: handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let handle::Event::State { state } = event
            && let Some(w) = app.windows.iter_mut().find(|w| &w.cosmic == proxy)
        {
            w.active = state.chunks_exact(4).any(|chunk| {
                u32::from_ne_bytes(chunk.try_into().unwrap()) == handle::State::Activated as u32
            });
            w.has_state = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_errors_remain_visible_when_collection_is_off() {
        let tracker = Tracker {
            control: Arc::new(Mutex::new(Control::default())),
            shared: Arc::new(Mutex::new(Shared {
                day: Day::empty(system_ms()),
                ready: true,
                error: Some("Protocol unavailable".into()),
                storage_error: Some("Retention failed".into()),
            })),
            stop: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(tracker.error(false).as_deref(), Some("Retention failed"));
        assert!(tracker.snapshot().unavailable);
        tracker.shared.lock().unwrap().storage_error = None;
        assert!(tracker.error(false).is_none());
        assert_eq!(tracker.error(true).as_deref(), Some("Protocol unavailable"));
    }

    #[test]
    fn missing_or_stale_supervisor_heartbeat_is_not_permission_to_record() {
        let control = Control {
            enabled: true,
            allowed: true,
            seen: Duration::from_secs(5),
        };
        assert!(control.fresh(Duration::from_secs(7)));
        assert!(!control.fresh(Duration::from_secs(9)));
        assert!(!control.fresh(Duration::from_secs(4)));
    }
}
