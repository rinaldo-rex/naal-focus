//! Opt-in foreground-window time. This measures window activation, not attention.
//! Only the current local day is retained; missing observations are gaps, never work.
use crate::time::to_local;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

pub mod store;
#[cfg(target_os = "linux")]
pub mod tracker;

pub const MAX_SESSIONS: usize = 20_000;
pub const MAX_TITLE_CHARS: usize = 256;
pub const MAX_APP_CHARS: usize = 128;
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub app_id: String,
    pub title: String,
}
impl Window {
    pub fn new(app_id: &str, title: &str) -> Self {
        let clean = |text: &str, limit| {
            text.chars()
                .filter(|c| !c.is_control())
                .take(limit)
                .collect()
        };
        Self {
            app_id: clean(app_id, MAX_APP_CHARS),
            title: clean(title, MAX_TITLE_CHARS),
        }
    }
    pub fn recordable(&self) -> bool {
        !self.app_id.is_empty()
            && self.app_id != crate::config::APP_ID
            && self.app_id != "naal-focus-overlay"
    }
    fn valid(&self) -> bool {
        self.recordable()
            && self.app_id.chars().count() <= MAX_APP_CHARS
            && self.title.chars().count() <= MAX_TITLE_CHARS
            && !self
                .app_id
                .chars()
                .chain(self.title.chars())
                .any(char::is_control)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub window: Window,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Measured with the monotonic boot clock, excluding gaps and suspend.
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Day {
    pub version: u32,
    pub date: String,
    pub observed_ms: u64,
    pub sessions: Vec<Session>,
}
impl Day {
    pub fn empty(now_ms: u64) -> Self {
        Self {
            version: 1,
            date: to_local(now_ms).date_key(),
            observed_ms: now_ms,
            sessions: Vec::new(),
        }
    }
    pub fn advance_day(&mut self, now_ms: u64) -> bool {
        if self.date == to_local(now_ms).date_key() {
            false
        } else {
            *self = Self::empty(now_ms);
            true
        }
    }
    pub fn validate(&self, now_ms: u64) -> Result<(), String> {
        if self.version != 1 || self.sessions.len() > MAX_SESSIONS {
            return Err("Unsupported or oversized focus history.".into());
        }
        let mut end = 0;
        for session in &self.sessions {
            if !session.window.valid()
                || session.start_ms >= session.end_ms
                || session.start_ms < end
                || session.end_ms > now_ms
                || session.duration_ms == 0
                || session.duration_ms
                    > (session.end_ms - session.start_ms)
                        .saturating_add(1000)
                        .saturating_add((session.end_ms - session.start_ms) / 1000)
                || to_local(session.start_ms).date_key() != self.date
                || to_local(session.end_ms.saturating_sub(1)).date_key() != self.date
            {
                return Err("Invalid focus history; recording is unavailable.".into());
            }
            end = session.end_ms;
        }
        if self.observed_ms > now_ms || end > self.observed_ms {
            return Err("Focus history contains future timestamps.".into());
        }
        Ok(())
    }
    pub fn total_ms(&self) -> u64 {
        self.sessions.iter().map(|s| s.duration_ms).sum()
    }
    /// Bound the ranking while conserving every measured millisecond.
    pub fn totals(&self, limit: usize) -> Vec<Total> {
        if limit == 0 {
            return Vec::new();
        }
        let mut values = BTreeMap::<Window, u64>::new();
        for session in &self.sessions {
            *values.entry(session.window.clone()).or_default() += session.duration_ms;
        }
        let mut values: Vec<_> = values
            .into_iter()
            .map(|(window, duration_ms)| Total {
                window: Some(window),
                duration_ms,
            })
            .collect();
        values.sort_by(|a, b| {
            b.duration_ms
                .cmp(&a.duration_ms)
                .then(a.window.cmp(&b.window))
        });
        if values.len() > limit {
            let duration_ms = values
                .drain(limit.saturating_sub(1)..)
                .map(|v| v.duration_ms)
                .sum();
            values.push(Total {
                window: None,
                duration_ms,
            });
        }
        values
    }
}

#[derive(Clone, Debug)]
pub struct Total {
    /// None is the aggregate Other row.
    pub window: Option<Window>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub day: Day,
    /// Unavailable is explicit: never present a failed tracker as zero usage.
    pub unavailable: bool,
}

/// Pure accounting. A stale guard, compositor stall, clock jump, or suspend
/// discards the uncertain interval. Restarts never bridge an unobserved gap.
#[derive(Default)]
pub struct Clock {
    previous: Option<(Duration, u64, Option<Window>, bool)>,
    fractional: Duration,
}
impl Clock {
    pub fn observe(
        &mut self,
        day: &mut Day,
        boot: Duration,
        wall: u64,
        window: Option<Window>,
        allowed: bool,
    ) -> Result<bool, String> {
        let rolled = day.advance_day(wall);
        let previous =
            self.previous
                .replace((boot, wall, window.filter(Window::recordable), allowed));
        day.observed_ms = wall;
        let Some((old_boot, old_wall, old_window, old_allowed)) = previous else {
            return Ok(rolled);
        };
        let elapsed = boot.saturating_sub(old_boot);
        if rolled
            || !allowed
            || !old_allowed
            || elapsed.is_zero()
            || elapsed > Duration::from_secs(3)
            || wall < old_wall
            || (wall - old_wall).abs_diff(elapsed.as_millis() as u64) > 1000
        {
            self.fractional = Duration::ZERO;
            return Ok(rolled);
        }
        let Some(window) = old_window else {
            self.fractional = Duration::ZERO;
            return Ok(rolled);
        };
        // A backwards clock must not create overlapping/duplicated history.
        if day.sessions.last().is_some_and(|s| old_wall < s.end_ms) {
            self.fractional = Duration::ZERO;
            return Ok(rolled);
        }
        // Busy compositor event streams can dispatch faster than 1 ms. Carry
        // fractions rather than rounding every dispatch down and losing time.
        let accumulated = elapsed + self.fractional;
        if accumulated > Duration::from_secs(3) {
            self.fractional = Duration::ZERO;
            return Ok(rolled);
        }
        let elapsed = accumulated.as_millis() as u64;
        if elapsed == 0 || wall == old_wall {
            self.fractional = accumulated;
            return Ok(rolled);
        }
        self.fractional = accumulated - Duration::from_millis(elapsed);
        if let Some(last) = day.sessions.last_mut()
            && last.window == window
            && last.end_ms == old_wall
        {
            last.end_ms = wall;
            last.duration_ms += elapsed;
        } else {
            if day.sessions.len() >= MAX_SESSIONS {
                return Err(
                    "Today's focus history is full; tracking paused until tomorrow.".into(),
                );
            }
            day.sessions.push(Session {
                window,
                start_ms: old_wall,
                end_ms: wall,
                duration_ms: elapsed,
            });
        }
        Ok(true)
    }
}

pub fn duration_label(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: u64 = 1_792_000_000_000;
    fn window(title: &str) -> Option<Window> {
        Some(Window::new("herdr", title))
    }
    fn sample(clock: &mut Clock, day: &mut Day, seconds: u64, title: &str, allowed: bool) {
        clock
            .observe(
                day,
                Duration::from_secs(seconds),
                START + seconds * 1000,
                window(title),
                allowed,
            )
            .unwrap();
    }
    #[test]
    fn switches_title_changes_and_repeats_are_measured_once() {
        let mut day = Day::empty(START);
        let mut clock = Clock::default();
        for (second, title) in [(0, "A"), (1, "A"), (2, "B"), (3, "B"), (4, "A"), (5, "A")] {
            sample(&mut clock, &mut day, second, title, true);
        }
        assert_eq!(day.sessions.len(), 3);
        assert_eq!(day.total_ms(), 5000);
        let totals = day.totals(4);
        assert_eq!(totals[0].window.as_ref().unwrap().title, "A");
        assert_eq!(totals[0].duration_ms, 3000);
        day.validate(START + 5000).unwrap();
    }
    #[test]
    fn unsafe_intervals_gaps_suspend_and_restart_never_count() {
        let mut day = Day::empty(START);
        let mut clock = Clock::default();
        for (second, safe) in [
            (0, true),
            (1, true),
            (2, false),
            (3, false),
            (4, true),
            (5, true),
            (100, true),
            (101, true),
        ] {
            sample(&mut clock, &mut day, second, "A", safe);
        }
        assert_eq!(day.total_ms(), 3000);
        assert_eq!(day.sessions.len(), 3);
        let mut restarted = Clock::default();
        sample(&mut restarted, &mut day, 200, "A", true);
        assert_eq!(day.total_ms(), 3000);
    }
    #[test]
    fn wall_clock_jumps_do_not_become_work_or_overlaps() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        sample(&mut clock, &mut day, 0, "A", true);
        sample(&mut clock, &mut day, 1, "A", true);
        clock
            .observe(
                &mut day,
                Duration::from_secs(2),
                START + 60_000,
                window("A"),
                true,
            )
            .unwrap();
        clock
            .observe(&mut day, Duration::from_secs(3), START, window("A"), true)
            .unwrap();
        clock
            .observe(
                &mut day,
                Duration::from_secs(4),
                START + 1000,
                window("A"),
                true,
            )
            .unwrap();
        assert_eq!(day.total_ms(), 1000);
    }
    #[test]
    fn fast_compositor_dispatches_do_not_round_away_window_time() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        for sample in 0..=10_000 {
            let micros = sample * 100;
            clock
                .observe(
                    &mut day,
                    Duration::from_micros(micros),
                    START + micros / 1000,
                    window("A"),
                    true,
                )
                .unwrap();
        }
        assert_eq!(day.total_ms(), 1000);
        assert_eq!(day.sessions.len(), 1);
        day.validate(day.observed_ms).unwrap();
    }

    #[test]
    fn small_clock_slews_keep_monotonic_time_and_remain_valid() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        for second in 0..4000 {
            clock
                .observe(
                    &mut day,
                    Duration::from_secs(second),
                    START + second * 999,
                    window("A"),
                    true,
                )
                .unwrap();
        }
        // This fixture may cross midnight; only the retained day is relevant.
        day.validate(day.observed_ms).unwrap();
        assert!(day.total_ms() > 0);
    }

    #[test]
    fn session_limit_is_explicit_and_a_new_day_recovers() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        for i in 0..=MAX_SESSIONS as u64 {
            clock
                .observe(
                    &mut day,
                    Duration::from_millis(i),
                    START + i,
                    window(&i.to_string()),
                    true,
                )
                .unwrap();
        }
        assert_eq!(day.sessions.len(), MAX_SESSIONS);
        assert!(
            clock
                .observe(
                    &mut day,
                    Duration::from_millis(MAX_SESSIONS as u64 + 1),
                    START + MAX_SESSIONS as u64 + 1,
                    window("full"),
                    true
                )
                .unwrap_err()
                .contains("full")
        );
        let next_day = START + 86_400_000;
        clock
            .observe(
                &mut day,
                Duration::from_millis(86_400_000),
                next_day,
                window("new"),
                true,
            )
            .unwrap();
        assert!(day.sessions.is_empty());
    }

    #[test]
    fn unknown_and_own_windows_leave_gaps() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        for (second, current) in [
            (0, window("A")),
            (1, window("A")),
            (2, None),
            (3, None),
            (4, Some(Window::new(crate::config::APP_ID, "Settings"))),
            (5, window("A")),
            (6, window("A")),
        ] {
            clock
                .observe(
                    &mut day,
                    Duration::from_secs(second),
                    START + second * 1000,
                    current,
                    true,
                )
                .unwrap();
        }
        assert_eq!(day.total_ms(), 3000);
        assert_eq!(day.sessions.len(), 2);
    }

    #[test]
    fn rollover_forgets_every_old_title_without_bridging_days() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        sample(&mut clock, &mut day, 0, "Sensitive title", true);
        sample(&mut clock, &mut day, 1, "Sensitive title", true);
        sample(&mut clock, &mut day, 86400, "New day", true);
        assert!(day.sessions.is_empty());
        sample(&mut clock, &mut day, 86401, "New day", true);
        assert_eq!(day.total_ms(), 1000);
        assert!(day.sessions.iter().all(|s| s.window.title == "New day"));
    }
    #[test]
    fn other_conserves_totals_and_own_windows_are_excluded() {
        let mut clock = Clock::default();
        let mut day = Day::empty(START);
        for second in 0..8 {
            sample(&mut clock, &mut day, second, &second.to_string(), true);
        }
        let totals = day.totals(4);
        assert_eq!(totals.len(), 4);
        assert!(totals.last().unwrap().window.is_none());
        assert_eq!(
            totals.iter().map(|r| r.duration_ms).sum::<u64>(),
            day.total_ms()
        );
        assert!(!Window::new(crate::config::APP_ID, "Settings").recordable());
        assert!(!Window::new("naal-focus-overlay", "Reminder").recordable());
    }
    #[test]
    fn metadata_is_bounded_and_validation_rejects_bad_records() {
        let w = Window::new(&"x".repeat(1000), &format!("a\n{}", "🪷".repeat(1000)));
        assert_eq!(w.app_id.len(), MAX_APP_CHARS);
        assert_eq!(w.title.chars().count(), MAX_TITLE_CHARS);
        assert!(!w.title.contains('\n'));
        let mut day = Day::empty(START);
        let mut clock = Clock::default();
        sample(&mut clock, &mut day, 0, "A", true);
        sample(&mut clock, &mut day, 1, "A", true);
        day.sessions[0].end_ms = START;
        assert!(day.validate(START + 1000).is_err());
    }
}
