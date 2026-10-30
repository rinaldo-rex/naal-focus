use crate::{process::boot_time, reminder::Observation};
use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_core_graphics::{
    CGEventSource, CGEventSourceStateID, CGEventType, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(super) struct Guard {
    pub latest: Arc<Mutex<Option<Observation>>>,
    stop: Arc<AtomicBool>,
}

impl Guard {
    pub fn start(idle_secs: u32) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, done) = (latest.clone(), stop.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                // Timestamp before sampling: a stalled platform call cannot refresh a stale result.
                let at = boot_time();
                let result = autoreleasepool(|_| sample(idle_secs));
                *shared.lock().unwrap() = Some(Observation { at, result });
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        Self { latest, stop }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sample(idle_secs: u32) -> Result<(), String> {
    let session = CGSessionCopyCurrentDictionary().ok_or("No graphical user session.")?;
    // SAFETY: CFDictionary and NSDictionary are toll-free bridged. The retained
    // CF dictionary outlives this reference; values are checked before use.
    let dictionary =
        unsafe { &*std::ptr::from_ref(&*session).cast::<NSDictionary<NSString, AnyObject>>() };
    let number = |key: &str| -> Result<Option<bool>, String> {
        dictionary
            .objectForKey(&NSString::from_str(key))
            .map(|value| {
                value
                    .downcast::<NSNumber>()
                    .map_err(|_| "Unrecognized desktop session state.".to_string())
                    .and_then(|value| match value.doubleValue() {
                        0.0 => Ok(false),
                        1.0 => Ok(true),
                        _ => Err("Unrecognized desktop session state.".into()),
                    })
            })
            .transpose()
    };
    // macOS omits the lock field when unlocked. This session-dictionary convention
    // is not a documented lock-state contract; live lock testing is mandatory.
    crate::reminder::session_ready(
        number("kCGSSessionOnConsoleKey")?,
        number("kCGSessionLoginDoneKey")?,
        number("CGSSessionScreenIsLocked")?,
        CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::HIDSystemState,
            CGEventType(u32::MAX),
        ),
        idle_secs,
    )
}
