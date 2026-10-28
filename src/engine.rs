//! A single supervisor thread owns the timer and at most one one-shot helper.
use crate::{
    config::{self, Settings},
    focus::{self, tracker::Tracker},
    guard::Guard,
    process::{ManagedChild, Singleton, boot_time, helper_command},
    schedule::Schedule,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use zbus::{
    blocking::Proxy,
    fdo::{RequestNameFlags, RequestNameReply},
};

pub const CONTROL_SERVICE: &str = "io.github.rinaldo_rex.NaalFocus.Control";
const CONTROL_PATH: &str = "/io/github/rinaldo_rex/NaalFocus/Control";
const CONTROL_INTERFACE: &str = "io.github.rinaldo_rex.NaalFocus.Control1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub settings: Settings,
    pub message: String,
    pub error: Option<String>,
    #[serde(default)]
    pub focus_error: Option<String>,
    pub running: bool,
    pub safe: bool,
    pub next_secs: u64,
    pub snoozed_secs: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Settings(Settings),
    Enabled(bool),
    Snooze(Duration),
    Resume,
    ShowNow,
    Shutdown,
}

pub struct Engine {
    tx: mpsc::Sender<Request>,
    status: Arc<Mutex<Status>>,
}
impl Engine {
    pub fn start(settings: Settings, path: Option<PathBuf>, load_error: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            settings: settings.clone(),
            message: "Starting safety checks…".into(),
            error: load_error.clone(),
            focus_error: None,
            running: false,
            safe: false,
            next_secs: settings.interval_secs,
            snoozed_secs: None,
        }));
        let shared = status.clone();
        let control_tx = tx.clone();
        std::thread::spawn(move || {
            if let Err(e) = share_supervisor(
                rx,
                control_tx,
                shared.clone(),
                settings,
                path,
                load_error.is_some(),
            ) {
                let mut status = shared.lock().unwrap();
                status.message = "Reminders unavailable.".into();
                status.error = Some(e);
                status.safe = false;
            }
        });
        Self { tx, status }
    }
    pub fn request(&self, request: Request) {
        let _ = self.tx.send(request);
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Shutdown);
    }
}

/// Each panel output is a client. Exactly one owns the D-Bus name and timer;
/// other outputs relay commands and read the same status without starting guards.
fn share_supervisor(
    rx: mpsc::Receiver<Request>,
    tx: mpsc::Sender<Request>,
    shared: Arc<Mutex<Status>>,
    mut settings: Settings,
    path: Option<PathBuf>,
    mut corrupt_config: bool,
) -> Result<(), String> {
    loop {
        let conn = zbus::blocking::connection::Builder::session()
            .map_err(|e| e.to_string())?
            .method_timeout(Duration::from_secs(1))
            // Initialize the object server inside zbus's blocking runtime,
            // not through object_server() on this plain supervisor thread.
            .serve_at(
                CONTROL_PATH,
                Control {
                    tx: tx.clone(),
                    status: shared.clone(),
                },
            )
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())?;
        match conn.request_name_with_flags(CONTROL_SERVICE, RequestNameFlags::DoNotQueue.into()) {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                // A former client reloads the shared settings before taking over.
                if let Some(path) = &path {
                    match config::load(path) {
                        Ok(loaded) => {
                            settings = loaded;
                            corrupt_config = false;
                        }
                        Err(error) => {
                            settings = Settings::default();
                            corrupt_config = true;
                            shared.lock().unwrap().error = Some(error);
                        }
                    }
                }
                {
                    let mut status = shared.lock().unwrap();
                    status.settings = settings.clone();
                    status.message = "Starting shared reminder controls…".into();
                    status.safe = false;
                    status.running = false;
                    if !corrupt_config {
                        status.error = None;
                    }
                }
                let result = supervise(
                    &rx,
                    shared.clone(),
                    settings.clone(),
                    path.clone(),
                    corrupt_config,
                );
                // Explicitly close the transport: object-server tasks hold clones.
                let _ = conn.close();
                match result {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        // During an upgrade the old owner can briefly retain the
                        // process lock. Do not permanently disconnect this popup.
                        let mut status = shared.lock().unwrap();
                        status.safe = false;
                        status.message = "Waiting for shared reminder controls…".into();
                        status.error = Some(error);
                        drop(status);
                        match rx.recv_timeout(Duration::from_millis(250)) {
                            Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                return Ok(());
                            }
                            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                    }
                }
            }
            Err(zbus::Error::NameTaken) | Ok(RequestNameReply::Exists) => {
                let proxy = Proxy::new(&conn, CONTROL_SERVICE, CONTROL_PATH, CONTROL_INTERFACE)
                    .map_err(|e| e.to_string())?;
                let reconnect = relay(&rx, &shared, &proxy);
                drop(proxy);
                let _ = conn.close();
                if !reconnect? {
                    return Ok(());
                }
                shared.lock().unwrap().message = "Reconnecting to shared reminders…".into();
            }
            Ok(other) => {
                return Err(format!(
                    "Unexpected control-service ownership reply: {other}"
                ));
            }
            Err(error) => return Err(format!("Cannot connect shared reminder controls: {error}")),
        }
    }
}

struct Control {
    tx: mpsc::Sender<Request>,
    status: Arc<Mutex<Status>>,
}
#[zbus::interface(name = "io.github.rinaldo_rex.NaalFocus.Control1")]
impl Control {
    fn snapshot(&self) -> zbus::fdo::Result<String> {
        serde_json::to_string(&*self.status.lock().unwrap())
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    fn send(&self, json: &str) -> zbus::fdo::Result<()> {
        let request = decode_request(json).map_err(zbus::fdo::Error::InvalidArgs)?;
        self.tx
            .send(request)
            .map_err(|_| zbus::fdo::Error::Failed("Supervisor stopped.".into()))
    }
}

fn decode_request(json: &str) -> Result<Request, String> {
    if json.len() > config::MAX_CONFIG_BYTES as usize {
        return Err("Control request is too large.".into());
    }
    let request: Request = serde_json::from_str(json).map_err(|e| e.to_string())?;
    match &request {
        Request::Shutdown => return Err("Only the owning applet can stop its supervisor.".into()),
        Request::Settings(settings) => settings.validate()?,
        Request::Snooze(duration)
            if *duration < Duration::from_secs(1) || *duration > Duration::from_secs(86400) =>
        {
            return Err("Snooze must be between one second and one day.".into());
        }
        _ => {}
    }
    Ok(request)
}

/// true asks the caller to elect a replacement; false means this UI is closing.
fn relay(
    rx: &mpsc::Receiver<Request>,
    shared: &Arc<Mutex<Status>>,
    proxy: &Proxy<'_>,
) -> Result<bool, String> {
    loop {
        let snapshot = proxy
            .call::<_, _, String>("Snapshot", &())
            .map_err(|e| e.to_string())
            .and_then(|json| serde_json::from_str::<Status>(&json).map_err(|e| e.to_string()));
        match snapshot {
            Ok(status) => *shared.lock().unwrap() = status,
            Err(error) => {
                let mut status = shared.lock().unwrap();
                status.safe = false;
                status.error = Some(format!("Shared controls reconnecting: {error}"));
                // Allow the previous owner's transport to finish closing. Do not
                // replay commands that might already have reached that owner.
                drop(status);
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Ok(false);
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => return Ok(true),
                }
            }
        }
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(false),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(request) => {
                let json = serde_json::to_string(&request).map_err(|e| e.to_string())?;
                if let Err(error) = proxy.call::<_, _, ()>("Send", &(json,)) {
                    shared.lock().unwrap().error = Some(format!("Shared control failed: {error}"));
                }
            }
        }
    }
}

fn supervise(
    rx: &mpsc::Receiver<Request>,
    shared: Arc<Mutex<Status>>,
    mut settings: Settings,
    path: Option<PathBuf>,
    mut corrupt_config: bool,
) -> Result<(), String> {
    settings.validate()?;
    let _singleton = Singleton::acquire("applet")?;
    let mut guard = Guard::start(settings.idle_secs);
    let tracker = Tracker::start();
    let mut schedule = Schedule::new(boot_time(), &settings);
    if let Some(seconds) = shared.lock().unwrap().snoozed_secs {
        schedule.snooze(boot_time(), Duration::from_secs(seconds), &settings);
    }
    let mut child: Option<(ManagedChild, Duration)> = None;
    let mut manual_at = None;
    let mut last_tick = boot_time();
    loop {
        let now = boot_time();
        let gap = now.saturating_sub(last_tick) > Duration::from_secs(3);
        last_tick = now;
        let blocked = guard.snapshot().blocked_reason(now);
        // This watchdog lives in the parent process, not the renderer. It also
        // kills a SIGSTOP'ed helper and counts time spent suspended.
        if let Some((helper, deadline)) = child.as_mut() {
            if blocked.is_some() || gap || now >= *deadline {
                child.take();
                shared.lock().unwrap().message = "Reminder cancelled by safety watchdog.".into();
            } else {
                match helper.0.try_wait() {
                    Ok(Some(exit)) => {
                        if !exit.success() {
                            shared.lock().unwrap().error =
                                Some(format!("Overlay exited with {exit}; recurrence paused."));
                            // Never repeatedly reopen a failing focus-capturing helper.
                            settings.enabled = false;
                            if let Some(path) = &path {
                                let _ = config::save(path, &settings);
                            }
                        }
                        child.take();
                    }
                    Ok(None) => {}
                    Err(e) => {
                        child.take();
                        shared.lock().unwrap().error = Some(e.to_string());
                        settings.enabled = false;
                    }
                }
            }
        }
        let safe = blocked.is_none();
        tracker.update(settings.focus.enabled, safe && child.is_none() && !gap);
        let due = schedule.tick(now, &settings, safe, child.is_some());
        let manual_due = manual_at.is_some_and(|at| now >= at);
        if manual_due {
            manual_at = None;
        }
        if due || manual_due {
            if safe && child.is_none() {
                // Suppress accounting before creating any focus-capturing surface.
                tracker.update(settings.focus.enabled, false);
                let snapshot = settings.focus.enabled.then(|| tracker.snapshot());
                let quote = schedule.take_quote(&settings);
                match launch(&settings, quote, snapshot) {
                    Ok(helper) => {
                        child = Some((
                            helper,
                            now + Duration::from_secs(settings.helper_lifetime_secs()),
                        ));
                        shared.lock().unwrap().error = None;
                    }
                    Err(e) => {
                        shared.lock().unwrap().error = Some(e);
                        settings.enabled = false;
                        if let Some(path) = &path {
                            let _ = config::save(path, &settings);
                        }
                    }
                }
                schedule.reset(now, &settings);
            } else if manual_due {
                shared.lock().unwrap().error = Some(
                    blocked
                        .clone()
                        .unwrap_or_else(|| "A reminder is already visible.".into()),
                );
            }
        }
        {
            let mut status = shared.lock().unwrap();
            status.settings = settings.clone();
            status.focus_error = tracker.error(settings.focus.enabled);
            status.running = child.is_some();
            status.safe = safe;
            status.next_secs = schedule.remaining(now).as_secs();
            status.snoozed_secs = schedule.snoozed_remaining(now).map(|d| d.as_secs());
            status.message = if child.is_some() {
                "A moment of awareness — all monitors.".into()
            } else if let Some(reason) = blocked {
                reason
            } else if manual_at.is_some() {
                "Showing on all monitors in a moment…".into()
            } else if !settings.enabled {
                "Reminders paused.".into()
            } else if let Some(secs) = status.snoozed_secs {
                format!("Snoozed — {} min remaining.", secs.div_ceil(60))
            } else {
                format!(
                    "Next reminder in {}:{:02}.",
                    status.next_secs / 60,
                    status.next_secs % 60
                )
            };
        }
        let request = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let now = boot_time();
        match request {
            Request::Shutdown => return Ok(()),
            Request::ShowNow => {
                if child.is_none() && safe {
                    manual_at = Some(now + Duration::from_secs(2));
                } else {
                    shared.lock().unwrap().error = Some(
                        "Cannot show a reminder while the session is unsafe or another is visible."
                            .into(),
                    );
                }
            }
            Request::Snooze(duration) => {
                child.take();
                manual_at = None;
                schedule.snooze(now, duration, &settings);
            }
            Request::Resume => {
                manual_at = None;
                schedule.resume(now, &settings);
            }
            request @ (Request::Settings(_) | Request::Enabled(_)) => {
                let explicit_save = matches!(request, Request::Settings(_));
                let candidate = match request {
                    Request::Settings(s) => s,
                    Request::Enabled(enabled) => Settings {
                        enabled,
                        ..settings.clone()
                    },
                    _ => unreachable!(),
                };
                let result = if corrupt_config && !explicit_save {
                    Err("Repair settings with Save before enabling reminders; the original file is preserved.".into())
                } else {
                    candidate
                        .validate()
                        .and_then(|_| {
                            path.as_ref()
                                .ok_or_else(|| "No writable settings path.".into())
                        })
                        .and_then(|path| config::save(path, &candidate))
                };
                match result {
                    Ok(()) => {
                        child.take();
                        manual_at = None;
                        if candidate.idle_secs != settings.idle_secs {
                            guard = Guard::start(candidate.idle_secs);
                        }
                        settings = candidate;
                        corrupt_config = false;
                        schedule.resume(now, &settings);
                        shared.lock().unwrap().error = None;
                    }
                    Err(e) => shared.lock().unwrap().error = Some(e),
                }
            }
        }
    }
}

fn helper_arguments(settings: &Settings) -> Vec<String> {
    let categories = settings
        .charts
        .kinds()
        .map(|kind| match kind {
            crate::metadata::Kind::Project => "projects",
            crate::metadata::Kind::Feeling => "feelings",
            crate::metadata::Kind::Tag => "tags",
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut arguments = vec![
        "--payload".into(),
        "--duration-secs".into(),
        settings.duration_secs.to_string(),
        "--typing-limit-secs".into(),
        settings.typing_limit_secs.to_string(),
        "--text-size".into(),
        settings.text_size.to_string(),
        "--idle-secs".into(),
        settings.idle_secs.to_string(),
        "--chart-style".into(),
        settings.charts.style.as_str().into(),
        "--charts".into(),
        if categories.is_empty() {
            "none".into()
        } else {
            categories
        },
        "--focus-style".into(),
        settings.focus.style.as_str().into(),
        "--focus-side".into(),
        settings.focus.side.as_str().into(),
        "--breathing".into(),
        if settings.breathing.enabled {
            "on"
        } else {
            "off"
        }
        .into(),
        "--breath-pace".into(),
        settings.breathing.pace.as_str().into(),
        "--breath-cycles".into(),
        settings.breathing.cycles.to_string(),
        "--breath-patterns".into(),
        settings
            .breathing
            .families
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>()
            .join(","),
    ];
    if settings.breathing.reduced_motion {
        arguments.push("--reduced-motion".into());
    }
    if settings.focus.enabled {
        arguments.push("--focus-history".into());
    }
    if settings.focus.use_24_hour_time {
        arguments.push("--focus-24-hour".into());
    }
    arguments
}

fn launch(
    settings: &Settings,
    quote: &str,
    focus: Option<focus::Snapshot>,
) -> Result<ManagedChild, String> {
    let mut command = helper_command()?;
    command
        .args(helper_arguments(settings))
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = ManagedChild(
        command
            .spawn()
            .map_err(|e| format!("Cannot launch overlay: {e}"))?,
    );
    // Private titles and quotes never appear in arguments or on D-Bus status.
    // A stopped reader must not stall the supervisor's safety watchdog.
    let mut stdin = child.0.stdin.take().ok_or("Helper stdin unavailable.")?;
    let payload = crate::overlay::Payload {
        quote: quote.to_owned(),
        focus,
    };
    std::thread::Builder::new()
        .name("overlay-payload".into())
        .spawn(move || {
            if serde_json::to_writer(&mut stdin, &payload).is_err() || stdin.flush().is_err() {
                eprintln!("Overlay payload transfer failed.");
            }
        })
        .map_err(|_| "Cannot start overlay payload transfer.")?;
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_receives_the_validated_typing_budget() {
        let settings = Settings {
            duration_secs: 30,
            typing_limit_secs: 600,
            text_size: 48,
            idle_secs: 900,
            ..Settings::default()
        };
        settings.validate().unwrap();
        let helper = crate::overlay::Options::parse(helper_arguments(&settings))
            .unwrap()
            .unwrap();
        assert!(helper.payload);
        assert!(!helper.stdin);
        assert_eq!(helper.settings.duration_secs, 30);
        assert_eq!(helper.settings.typing_limit_secs, 600);
        assert_eq!(helper.settings.text_size, 48);
        assert_eq!(helper.settings.idle_secs, 900);
        assert_eq!(settings.helper_lifetime_secs(), 652);
        assert_eq!(
            helper.settings.helper_lifetime_secs(),
            settings.helper_lifetime_secs()
        );
    }

    #[test]
    fn helper_receives_every_breathing_preference_and_watchdog_budget() {
        use crate::breathing::{Family, Pace};
        for mask in 1..16 {
            for enabled in [false, true] {
                for pace in Pace::ALL {
                    for cycles in [1, 2] {
                        let settings = Settings {
                            breathing: crate::breathing::Settings {
                                enabled,
                                pace,
                                cycles,
                                reduced_motion: mask % 2 == 0,
                                families: Family::ALL
                                    .into_iter()
                                    .enumerate()
                                    .filter_map(|(i, f)| (mask & (1 << i) != 0).then_some(f))
                                    .collect(),
                            },
                            ..Settings::default()
                        };
                        settings.validate().unwrap();
                        let helper = crate::overlay::Options::parse(helper_arguments(&settings))
                            .unwrap()
                            .unwrap();
                        assert_eq!(helper.settings.breathing, settings.breathing);
                        assert_eq!(
                            helper.settings.helper_lifetime_secs(),
                            settings.helper_lifetime_secs()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn helper_receives_every_focus_preference_without_private_data_in_arguments() {
        for style in [
            crate::config::FocusStyle::Totals,
            crate::config::FocusStyle::Timeline,
            crate::config::FocusStyle::Both,
        ] {
            for side in [
                crate::config::FocusSide::Left,
                crate::config::FocusSide::Right,
            ] {
                for enabled in [false, true] {
                    for use_24_hour_time in [false, true] {
                        let settings = Settings {
                            focus: crate::config::FocusSettings {
                                enabled,
                                style,
                                side,
                                use_24_hour_time,
                            },
                            ..Settings::default()
                        };
                        let arguments = helper_arguments(&settings);
                        assert_eq!(arguments[0], "--payload");
                        let helper = crate::overlay::Options::parse(arguments).unwrap().unwrap();
                        assert_eq!(helper.settings.focus, settings.focus);
                    }
                }
            }
        }
    }

    #[test]
    fn helper_receives_chart_style_and_every_category_combination() {
        for style in [
            crate::config::ChartStyle::Heatmap,
            crate::config::ChartStyle::Treemap,
        ] {
            for mask in 0..8 {
                let settings = Settings {
                    charts: crate::config::ChartSettings {
                        style,
                        projects: mask & 1 != 0,
                        feelings: mask & 2 != 0,
                        tags: mask & 4 != 0,
                    },
                    ..Settings::default()
                };
                let helper = crate::overlay::Options::parse(helper_arguments(&settings))
                    .unwrap()
                    .unwrap();
                assert_eq!(helper.settings.charts, settings.charts);
            }
        }
    }

    #[test]
    fn remote_commands_cannot_stop_the_owner_or_bypass_validation() {
        for request in [
            Request::Shutdown,
            Request::Snooze(Duration::ZERO),
            Request::Snooze(Duration::from_nanos(1)),
            Request::Snooze(Duration::from_secs(86401)),
            Request::Settings(Settings {
                duration_secs: 0,
                ..Settings::default()
            }),
            Request::Settings(Settings {
                typing_limit_secs: 601,
                ..Settings::default()
            }),
        ] {
            assert!(decode_request(&serde_json::to_string(&request).unwrap()).is_err());
        }
        for request in [
            Request::Enabled(false),
            Request::Resume,
            Request::Snooze(Duration::from_secs(900)),
        ] {
            assert!(decode_request(&serde_json::to_string(&request).unwrap()).is_ok());
        }
        assert!(decode_request(&"x".repeat(config::MAX_CONFIG_BYTES as usize + 1)).is_err());
    }
}
