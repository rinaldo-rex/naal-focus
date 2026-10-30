use super::safety::Guard;
use crate::{
    config::{self, Settings},
    pending::Pending,
    process::boot_time,
    reminder,
    schedule::Schedule,
};
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Clone)]
pub(super) struct Status {
    pub settings: Settings,
    pub message: String,
    pub error: Option<String>,
    pub saving: bool,
    pub snoozed: bool,
}

pub(super) enum Request {
    Settings(Settings),
    Snooze,
    Resume,
    ShowNow,
    Interrupt,
    Stop,
}

pub(super) struct Engine {
    tx: mpsc::Sender<Request>,
    status: Arc<Mutex<Status>>,
}

impl Engine {
    pub fn start(settings: Settings, path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            settings: settings.clone(),
            message: "Checking desktop safety…".into(),
            error: None,
            saving: false,
            snoozed: false,
        }));
        let shared = status.clone();
        std::thread::spawn(move || supervise(rx, shared, settings, path));
        Self { tx, status }
    }
    pub fn request(&self, request: Request) {
        if matches!(request, Request::Settings(_)) {
            let mut status = self.status.lock().unwrap();
            if status.saving {
                return;
            }
            status.saving = true;
            status.error = None;
        }
        if self.tx.send(request).is_err() {
            let mut status = self.status.lock().unwrap();
            status.saving = false;
            status.error = Some("Reminder supervisor stopped.".into());
        }
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.request(Request::Stop);
    }
}

fn supervise(
    rx: mpsc::Receiver<Request>,
    shared: Arc<Mutex<Status>>,
    mut settings: Settings,
    path: PathBuf,
) {
    let mut schedule = Schedule::new(boot_time(), &settings);
    let mut guard = Guard::start(settings.idle_secs);
    let mut helper: Option<Helper> = None;
    let mut show_at = None;
    let mut save: Option<Pending<Settings>> = None;
    loop {
        let now = boot_time();
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Request::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Ok(Request::Settings(draft)) => {
                if save.is_none() {
                    let path = path.clone();
                    match Pending::start("macos-save-settings", move || {
                        config::save(&path, &draft)?;
                        Ok(draft)
                    }) {
                        Ok(job) => {
                            save = Some(job);
                            let mut status = shared.lock().unwrap();
                            status.saving = true;
                            status.error = None;
                        }
                        Err(error) => shared.lock().unwrap().error = Some(error.to_string()),
                    }
                }
            }
            Ok(Request::Snooze) => {
                schedule.snooze(now, Duration::from_secs(900), &settings);
                show_at = None;
                helper = None;
            }
            Ok(Request::Resume) => schedule.resume(now, &settings),
            Ok(Request::Interrupt) => {
                helper = None;
                show_at = None;
                schedule.reset(now, &settings);
                guard = Guard::start(settings.idle_secs);
            }
            Ok(Request::ShowNow) if helper.is_none() => {
                show_at = Some(now + Duration::from_secs(2));
            }
            Ok(Request::ShowNow) | Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = boot_time();
        if let Some(result) = save
            .as_ref()
            .and_then(|job| job.poll("Settings worker stopped."))
        {
            save = None;
            let mut status = shared.lock().unwrap();
            status.saving = false;
            match result {
                Ok(saved) => {
                    guard = Guard::start(saved.idle_secs);
                    settings = saved;
                    schedule.reset(now, &settings);
                    show_at = None;
                    helper = None;
                    status.settings = settings.clone();
                    status.error = None;
                }
                Err(error) => status.error = Some(error),
            }
        }
        let blocked = reminder::blocked(guard.latest.lock().unwrap().as_ref(), now);
        if let Some(child) = &mut helper {
            match child.tick(now, blocked.is_none()) {
                Ok(true) => helper = None,
                Ok(false) => {}
                Err(error) => {
                    helper = None;
                    shared.lock().unwrap().error = Some(error);
                }
            }
        }
        let scheduled = schedule.tick(now, &settings, blocked.is_none(), helper.is_some());
        let manual = show_at.is_some_and(|at| now >= at);
        if manual {
            show_at = None;
        }
        if (scheduled || manual) && blocked.is_none() && helper.is_none() && save.is_none() {
            let mut payload = settings.clone();
            payload.reflections_enabled = false;
            payload.charts = Default::default();
            payload.focus = Default::default();
            payload.quotes = vec![schedule.take_quote(&settings).to_owned()];
            match Helper::start(&payload, now) {
                Ok(child) => {
                    helper = Some(child);
                    shared.lock().unwrap().error = None;
                }
                Err(error) => shared.lock().unwrap().error = Some(error),
            }
        }
        let mut status = shared.lock().unwrap();
        status.snoozed = schedule.snoozed_remaining(now).is_some();
        status.message = if let Some(reason) = blocked {
            reason
        } else if helper.is_some() {
            "Reminder active".into()
        } else if show_at.is_some() {
            "Showing in 2 seconds…".into()
        } else if let Some(remaining) = schedule.snoozed_remaining(now) {
            format!("Snoozed · {} min", remaining.as_secs().div_ceil(60))
        } else if !settings.enabled {
            "Paused".into()
        } else {
            format!("Next reminder · {} sec", schedule.remaining(now).as_secs())
        };
    }
}

fn nonblocking(fd: &impl AsRawFd) -> Result<(), String> {
    // SAFETY: the caller retains the live descriptor; fcntl only changes its flags.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    Ok(())
}

struct Helper {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    payload: Vec<u8>,
    written: usize,
    started: Duration,
    lifetime: Duration,
    ready: bool,
}
impl Helper {
    fn start(settings: &Settings, now: Duration) -> Result<Self, String> {
        let mut payload = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
        payload.push(b'\n');
        let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .arg("--overlay")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child.stdin.take().expect("piped input");
        let output = child.stdout.take().expect("piped output");
        // Own the child before fallible initialization: every failure kills and reaps it.
        let helper = Self {
            child,
            input,
            output,
            payload,
            written: 0,
            started: now,
            lifetime: Duration::from_secs(settings.helper_lifetime_secs()),
            ready: false,
        };
        nonblocking(&helper.input)?;
        nonblocking(&helper.output)?;
        Ok(helper)
    }
    fn tick(&mut self, now: Duration, safe: bool) -> Result<bool, String> {
        if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(true)
            } else {
                Err(format!("Reminder helper exited unsuccessfully: {status}"))
            };
        }
        if !safe {
            return Ok(true);
        }
        if now.saturating_sub(self.started) >= self.lifetime {
            return Err("Reminder exceeded its safety deadline.".into());
        }
        if !self.ready {
            let mut byte = [0];
            match self.output.read(&mut byte) {
                Ok(1) if byte[0] == b'R' => self.ready = true,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Ok(_) => return Err("Reminder exited before all monitors were ready.".into()),
                Err(error) => return Err(error.to_string()),
            }
            if !self.ready && now.saturating_sub(self.started) > Duration::from_secs(3) {
                return Err("Not every monitor became ready within 3 seconds.".into());
            }
        }
        let bytes = if self.written < self.payload.len() {
            &self.payload[self.written..]
        } else {
            b"H"
        };
        match self.input.write(bytes) {
            Ok(0) => return Err("Reminder input closed.".into()),
            Ok(count) => self.written = (self.written + count).min(self.payload.len()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.to_string()),
        }
        Ok(false)
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
