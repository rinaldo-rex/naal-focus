//! A single supervisor thread owns the timer and at most one one-shot helper.
use crate::{
    config::{self, Settings},
    guard::Guard,
    process::{ManagedChild, Singleton, boot_time, helper_command},
    schedule::Schedule,
};
use std::{
    io::Write,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Status {
    pub settings: Settings,
    pub message: String,
    pub error: Option<String>,
    pub running: bool,
    pub safe: bool,
    pub next_secs: u64,
    pub snoozed_secs: Option<u64>,
}

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
            running: false,
            safe: false,
            next_secs: settings.interval_secs,
            snoozed_secs: None,
        }));
        let shared = status.clone();
        std::thread::spawn(move || {
            if let Err(e) = supervise(rx, shared.clone(), settings, path, load_error.is_some()) {
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

fn supervise(
    rx: mpsc::Receiver<Request>,
    shared: Arc<Mutex<Status>>,
    mut settings: Settings,
    path: Option<PathBuf>,
    mut corrupt_config: bool,
) -> Result<(), String> {
    let _singleton = Singleton::acquire("applet")?;
    let mut guard = Guard::start(settings.idle_secs);
    let mut schedule = Schedule::new(boot_time(), &settings);
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
        let due = schedule.tick(now, &settings, safe, child.is_some());
        let manual_due = manual_at.is_some_and(|at| now >= at);
        if manual_due {
            manual_at = None;
        }
        if due || manual_due {
            if safe && child.is_none() {
                let quote = schedule.take_quote(&settings);
                match launch(&settings, quote) {
                    Ok(helper) => {
                        child = Some((
                            helper,
                            now + Duration::from_secs(settings.duration_secs + 8),
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

fn launch(settings: &Settings, quote: &str) -> Result<ManagedChild, String> {
    let mut command = helper_command()?;
    command
        .args([
            "--duration-secs",
            &settings.duration_secs.to_string(),
            "--text-size",
            &settings.text_size.to_string(),
            "--idle-secs",
            &settings.idle_secs.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let mut child = ManagedChild(
        command
            .spawn()
            .map_err(|e| format!("Cannot launch overlay: {e}"))?,
    );
    // Quotes are sent as bounded UTF-8 on stdin, not visible in process arguments.
    let mut stdin = child.0.stdin.take().ok_or("Helper stdin unavailable.")?;
    stdin
        .write_all(quote.as_bytes())
        .map_err(|e| e.to_string())?;
    drop(stdin);
    Ok(child)
}
