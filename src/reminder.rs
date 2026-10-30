// Admission and bounds are testable without creating desktop surfaces.
#![cfg(any(all(target_os = "macos", feature = "applet"), test))]

use std::time::Duration;

pub(crate) const SAFETY_MAX_AGE: Duration = Duration::from_secs(3);
pub(crate) const MAX_DISPLAY_PIXELS: usize = 32 * 1024 * 1024;
pub(crate) const MAX_TOTAL_PIXELS: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub at: Duration,
    pub result: Result<(), String>,
}

pub(crate) fn blocked(sample: Option<&Observation>, now: Duration) -> Option<String> {
    match sample {
        None => Some("Checking desktop safety…".into()),
        Some(sample) if now < sample.at || now - sample.at > SAFETY_MAX_AGE => {
            Some("Desktop safety unavailable or stale.".into())
        }
        Some(sample) => sample.result.as_ref().err().cloned(),
    }
}

pub(crate) fn session_ready(
    on_console: Option<bool>,
    login_done: Option<bool>,
    locked: Option<bool>,
    idle: f64,
    idle_secs: u32,
) -> Result<(), String> {
    if on_console != Some(true) || login_done != Some(true) {
        return Err("Desktop session inactive or asleep.".into());
    }
    if locked.unwrap_or(false) {
        return Err("Screen locked.".into());
    }
    if !idle.is_finite() || idle < 0.0 {
        return Err("Desktop idle state unavailable.".into());
    }
    if idle >= f64::from(idle_secs) {
        return Err("Waiting for activity; desktop is idle.".into());
    }
    Ok(())
}

pub(crate) fn pixel_size(width: f64, height: f64, scale: f64) -> Result<(usize, usize), String> {
    if !width.is_finite()
        || !height.is_finite()
        || !scale.is_finite()
        || width <= 0.0
        || height <= 0.0
        || scale < 1.0
    {
        return Err("Invalid display size or scale.".into());
    }
    let width = (width * scale).round() as usize;
    let height = (height * scale).round() as usize;
    let pixels = width.checked_mul(height).ok_or("Display is too large.")?;
    if pixels == 0 || pixels > MAX_DISPLAY_PIXELS {
        return Err("Display exceeds the reminder memory limit.".into());
    }
    Ok((width, height))
}

pub(crate) fn watchdog_expired(
    now: Duration,
    started: Duration,
    heartbeat: Duration,
    lifetime: Duration,
) -> bool {
    now < started
        || now < heartbeat
        || now.saturating_sub(started) >= lifetime
        || now.saturating_sub(heartbeat) > SAFETY_MAX_AGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_failed_future_and_stale_observations_block() {
        let now = Duration::from_secs(10);
        assert!(blocked(None, now).is_some());
        let mut sample = Observation {
            at: now,
            result: Ok(()),
        };
        assert!(blocked(Some(&sample), now).is_none());
        sample.result = Err("Locked".into());
        assert_eq!(blocked(Some(&sample), now).as_deref(), Some("Locked"));
        sample.result = Ok(());
        assert!(
            blocked(
                Some(&sample),
                now + SAFETY_MAX_AGE + Duration::from_millis(1)
            )
            .is_some()
        );
        assert!(blocked(Some(&sample), now - Duration::from_secs(1)).is_some());
    }

    #[test]
    fn native_session_requires_active_console_login_and_valid_idle_state() {
        assert!(session_ready(Some(true), Some(true), None, 0.0, 120).is_ok());
        for (console, login, locked, idle) in [
            (None, Some(true), None, 0.0),
            (Some(true), None, None, 0.0),
            (Some(false), Some(true), None, 0.0),
            (Some(true), Some(true), Some(true), 0.0),
            (Some(true), Some(true), None, f64::NAN),
            (Some(true), Some(true), None, f64::INFINITY),
            (Some(true), Some(true), None, -1.0),
            (Some(true), Some(true), None, 120.0),
        ] {
            assert!(session_ready(console, login, locked, idle, 120).is_err());
        }
    }

    #[test]
    fn desktop_pixels_respect_scale_and_memory_bounds() {
        for (width, height, scale) in [
            (1000.0, 660.0, 1.0),
            (1920.0, 1080.0, 1.0),
            (1707.0, 960.0, 2.0),
        ] {
            let size = pixel_size(width, height, scale).unwrap();
            assert_eq!(size, ((width * scale) as usize, (height * scale) as usize));
            assert!(size.0 * size.1 <= MAX_TOTAL_PIXELS);
        }
        for (width, height, scale) in [
            (0.0, 1080.0, 1.0),
            (f64::NAN, 1080.0, 1.0),
            (1920.0, 1080.0, f64::INFINITY),
            (1e100, 1e100, 2.0),
        ] {
            assert!(pixel_size(width, height, scale).is_err());
        }
    }

    #[test]
    fn watchdog_cannot_be_extended_by_input_or_a_live_parent() {
        let start = Duration::from_secs(10);
        let limit = Duration::from_secs(20);
        assert!(!watchdog_expired(start, start, start, limit));
        assert!(watchdog_expired(
            start + Duration::from_secs(4),
            start,
            start,
            limit
        ));
        assert!(watchdog_expired(start + limit, start, start + limit, limit));
        assert!(watchdog_expired(Duration::ZERO, start, start, limit));
    }
}
