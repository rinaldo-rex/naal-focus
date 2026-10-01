//! Fail-closed desktop safety checks. No screensaver inhibitors are requested.
use crate::process::{boot_time, desktop_connection, runtime_dir};
use smithay_client_toolkit::reexports::{
    calloop::EventLoop, calloop_wayland_source::WaylandSource,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};
use zbus::{
    blocking::{Connection as BusConnection, Proxy as BusProxy},
    zvariant::OwnedObjectPath,
};

#[derive(Clone, Debug)]
pub struct Safety {
    pub session_reason: Option<String>,
    pub session_seen: Option<Duration>,
    pub idle: bool,
    pub idle_seen: Option<Duration>,
    pub idle_error: Option<String>,
}
impl Default for Safety {
    fn default() -> Self {
        Self {
            session_reason: Some("Checking session safety…".into()),
            session_seen: None,
            idle: false,
            idle_seen: None,
            idle_error: Some("Checking desktop idle protocol…".into()),
        }
    }
}
impl Safety {
    pub fn blocked_reason(&self, now: Duration) -> Option<String> {
        if self
            .session_seen
            .is_none_or(|t| now.saturating_sub(t) > Duration::from_secs(3))
        {
            return Some("Session safety unavailable or stale; reminders blocked.".into());
        }
        if let Some(reason) = &self.session_reason {
            return Some(reason.clone());
        }
        if self
            .idle_seen
            .is_none_or(|t| now.saturating_sub(t) > Duration::from_secs(3))
        {
            return Some("Desktop idle monitor unavailable or stale; reminders blocked.".into());
        }
        if let Some(reason) = &self.idle_error {
            return Some(reason.clone());
        }
        self.idle
            .then(|| "Waiting for activity; desktop is idle.".into())
    }
}

pub struct Guard {
    state: Arc<Mutex<Safety>>,
    stop: Arc<AtomicBool>,
}
impl Guard {
    pub fn start(idle_secs: u32) -> Self {
        let state = Arc::new(Mutex::new(Safety::default()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let result = session_loop(&state, &stop);
                    if let Err(e) = result {
                        state.lock().unwrap().session_reason =
                            Some(format!("Session guard failed: {e}"));
                    }
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            });
        }
        {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                if let Err(e) = idle_loop(state.clone(), stop, idle_secs) {
                    state.lock().unwrap().idle_error = Some(format!("Idle guard failed: {e}"));
                }
            });
        }
        Self { state, stop }
    }
    pub fn snapshot(&self) -> Safety {
        self.state.lock().unwrap().clone()
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn session_loop(
    state: &Arc<Mutex<Safety>>,
    stop: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = zbus::blocking::connection::Builder::system()?
        .method_timeout(Duration::from_secs(1))
        .build()?;
    let manager = BusProxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    // The panel can be launched through a user service, outside a logind session.
    // Resolve the current user's graphical Display session, not the applet PID.
    let uid = unsafe { libc::getuid() }; // SAFETY: getuid has no arguments or preconditions.
    let user: OwnedObjectPath = manager.call("GetUser", &(uid,))?;
    let user_proxy = BusProxy::new(
        &conn,
        "org.freedesktop.login1",
        user.as_str(),
        "org.freedesktop.login1.User",
    )?;
    let (session_id, session_path): (String, OwnedObjectPath) =
        user_proxy.get_property("Display")?;
    if session_id.is_empty() {
        return Err("No graphical logind session.".into());
    }
    let lockfile = runtime_dir()?.join(format!("cosmic-greeter-{session_id}.lock"));
    while !stop.load(Ordering::Relaxed) {
        let reason = session_reason(&conn, &session_path, &lockfile)?;
        {
            let mut status = state.lock().unwrap();
            status.session_reason = reason;
            status.session_seen = Some(boot_time());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn session_reason(
    conn: &BusConnection,
    path: &OwnedObjectPath,
    lockfile: &std::path::Path,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // New proxies each poll avoid ever relying on cached property values.
    let manager = BusProxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let session = BusProxy::new(
        conn,
        "org.freedesktop.login1",
        path.as_str(),
        "org.freedesktop.login1.Session",
    )?;
    let preparing: bool = manager.get_property("PreparingForSleep")?;
    let active: bool = session.get_property("Active")?;
    let locked: bool = session.get_property("LockedHint")?;
    let idle: bool = session.get_property("IdleHint")?;
    let kind: String = session.get_property("Type")?;
    // COSMIC's locker currently maintains this marker instead of setting LockedHint.
    // Treat any metadata error other than NotFound as unsafe.
    let cosmic_locked = match std::fs::metadata(lockfile) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    Ok(if preparing {
        Some("Waiting; system is preparing to sleep.".into())
    } else if !active || kind != "wayland" {
        Some("Waiting; graphical session is inactive.".into())
    } else if locked || cosmic_locked {
        Some("Waiting; screen is locked.".into())
    } else if idle {
        Some("Waiting for activity; session is idle.".into())
    } else {
        None
    })
}

struct IdleState {
    seats: HashMap<u32, bool>,
    state: Arc<Mutex<Safety>>,
}
impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, u32> for IdleState {
    fn event(
        app: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        seat: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                app.seats.insert(*seat, true);
            }
            ext_idle_notification_v1::Event::Resumed => {
                app.seats.insert(*seat, false);
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for IdleState {
    fn event(
        app: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_registry::Event::GlobalRemove { name } if app.seats.contains_key(&name))
            || matches!(event, wl_registry::Event::Global { ref interface, .. } if interface == "wl_seat")
        {
            // A changed seat topology needs a fresh guard; never silently assume activity.
            app.state.lock().unwrap().idle_error =
                Some("Input seats changed; restart the applet.".into());
        }
    }
}
delegate_noop!(IdleState: ignore wl_seat::WlSeat);
delegate_noop!(IdleState: ignore ext_idle_notifier_v1::ExtIdleNotifierV1);

fn idle_loop(
    state: Arc<Mutex<Safety>>,
    stop: Arc<AtomicBool>,
    timeout: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = desktop_connection()?;
    let (globals, mut queue) = registry_queue_init::<IdleState>(&conn)?;
    let qh = queue.handle();
    // v2 tracks input even when another app (e.g. a video player) inhibits idle.
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals.bind(&qh, 2..=2, ())?;
    let mut app = IdleState {
        seats: HashMap::new(),
        state: state.clone(),
    };
    let mut notifications = Vec::new();
    for global in globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wl_seat")
    {
        let seat: wl_seat::WlSeat =
            globals
                .registry()
                .bind(global.name, global.version.min(7), &qh, ());
        let notification =
            notifier.get_input_idle_notification(timeout * 1000, &seat, &qh, global.name);
        app.seats.insert(global.name, false);
        notifications.push((seat, notification));
    }
    if notifications.is_empty() {
        return Err("No input seat found.".into());
    }
    queue.roundtrip(&mut app)?;
    {
        let mut status = state.lock().unwrap();
        status.idle_error = None;
    }
    let mut event_loop: EventLoop<IdleState> = EventLoop::try_new()?;
    WaylandSource::new(conn, queue).insert(event_loop.handle())?;
    while !stop.load(Ordering::Relaxed) {
        event_loop.dispatch(Duration::from_millis(100), &mut app)?;
        let mut status = state.lock().unwrap();
        status.idle = app.seats.values().all(|idle| *idle);
        status.idle_seen = Some(boot_time());
    }
    for (seat, notification) in notifications {
        notification.destroy();
        seat.release();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safety_is_fail_closed_and_expires() {
        let now = Duration::from_secs(10);
        let mut s = Safety::default();
        assert!(s.blocked_reason(now).is_some());
        s.session_seen = Some(now);
        s.session_reason = None;
        s.idle_seen = Some(now);
        s.idle_error = None;
        assert!(s.blocked_reason(now).is_none());
        s.idle = true;
        assert!(s.blocked_reason(now).is_some());
        s.idle = false;
        assert!(s.blocked_reason(now + Duration::from_secs(4)).is_some());
        s.session_reason = Some("locked".into());
        assert_eq!(s.blocked_reason(now).as_deref(), Some("locked"));
    }
}
