use crate::config::Settings;
use std::time::Duration;

/// Pure scheduling policy. Time is CLOCK_BOOTTIME, so it includes suspend.
#[derive(Debug)]
pub struct Schedule {
    next: Duration,
    last_tick: Duration,
    snoozed_until: Option<Duration>,
    quote_index: usize,
    was_safe: bool,
}

impl Schedule {
    pub fn new(now: Duration, settings: &Settings) -> Self {
        Self {
            next: now + interval(settings),
            last_tick: now,
            snoozed_until: None,
            quote_index: 0,
            was_safe: false,
        }
    }

    pub fn reset(&mut self, now: Duration, settings: &Settings) {
        self.next = now + interval(settings);
        self.last_tick = now;
    }

    pub fn snooze(&mut self, now: Duration, duration: Duration, settings: &Settings) {
        self.snoozed_until = Some(now + duration);
        self.reset(now + duration, settings);
        self.last_tick = now;
    }

    pub fn resume(&mut self, now: Duration, settings: &Settings) {
        self.snoozed_until = None;
        self.reset(now, settings);
    }

    pub fn snoozed_remaining(&self, now: Duration) -> Option<Duration> {
        self.snoozed_until
            .filter(|end| *end > now)
            .map(|end| end - now)
    }

    pub fn remaining(&self, now: Duration) -> Duration {
        self.next.saturating_sub(now)
    }

    /// A gap, unsafe session, or recovery starts a full interval. Never catch up.
    pub fn tick(
        &mut self,
        now: Duration,
        settings: &Settings,
        safe: bool,
        child_active: bool,
    ) -> bool {
        let gap = now.saturating_sub(self.last_tick) > Duration::from_secs(3);
        self.last_tick = now;
        if gap || !safe || !self.was_safe {
            self.next = now + interval(settings);
        }
        self.was_safe = safe;
        // A slow helper must not leave an overdue reminder queued for its exit.
        if child_active && now >= self.next {
            self.next = now + interval(settings);
        }
        if !settings.enabled || !safe || child_active || self.snoozed_remaining(now).is_some() {
            return false;
        }
        if now >= self.next {
            self.next = now + interval(settings);
            return true;
        }
        false
    }

    pub fn take_quote<'a>(&mut self, settings: &'a Settings) -> &'a str {
        let quote = &settings.quotes[self.quote_index % settings.quotes.len()];
        self.quote_index = self.quote_index.wrapping_add(1);
        quote
    }
}

fn interval(settings: &Settings) -> Duration {
    Duration::from_secs(settings.interval_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }
    fn settings() -> Settings {
        Settings {
            enabled: true,
            interval_secs: 15,
            ..Settings::default()
        }
    }
    fn tick_to(s: &mut Schedule, cfg: &Settings, start: u64, end: u64) -> bool {
        (start..=end)
            .map(|t| s.tick(at(t), cfg, true, false))
            .fold(false, |any, due| any | due)
    }

    #[test]
    fn waits_a_full_interval_and_never_overlaps() {
        let cfg = settings();
        let mut s = Schedule::new(at(0), &cfg);
        assert!(!tick_to(&mut s, &cfg, 0, 14));
        assert!(s.tick(at(15), &cfg, true, false));
        for t in 16..=30 {
            assert!(!s.tick(at(t), &cfg, true, true));
        }
        assert!(!tick_to(&mut s, &cfg, 31, 44));
        assert!(s.tick(at(45), &cfg, true, false));
        assert!(!s.tick(at(45), &cfg, true, false));
    }

    #[test]
    fn pause_snooze_resume_and_quotes() {
        let mut cfg = settings();
        let mut s = Schedule::new(at(0), &cfg);
        cfg.enabled = false;
        assert!(!tick_to(&mut s, &cfg, 0, 20));
        cfg.enabled = true;
        s.snooze(at(20), at(10), &cfg);
        assert!(!tick_to(&mut s, &cfg, 21, 44));
        assert!(s.tick(at(45), &cfg, true, false));
        s.resume(at(45), &cfg);
        assert!(!tick_to(&mut s, &cfg, 46, 59));
        assert!(s.tick(at(60), &cfg, true, false));
        for i in 0..10 {
            assert_eq!(s.take_quote(&cfg), cfg.quotes[i % cfg.quotes.len()]);
        }
    }

    #[test]
    fn unsafe_sessions_and_resume_gaps_reset_without_bursts() {
        let cfg = settings();
        let mut s = Schedule::new(at(0), &cfg);
        tick_to(&mut s, &cfg, 0, 10);
        assert!(!s.tick(at(11), &cfg, false, false));
        assert!(!s.tick(at(12), &cfg, true, false));
        assert!(!tick_to(&mut s, &cfg, 13, 26));
        assert!(s.tick(at(27), &cfg, true, false));
        assert!(!s.tick(at(1000), &cfg, true, false));
        assert!(!tick_to(&mut s, &cfg, 1001, 1014));
        assert!(s.tick(at(1015), &cfg, true, false));
    }
}
