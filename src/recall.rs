//! Today's chronological reflection dial. No writes; reuse always submits a
//! new moment through the existing activity editor and persistence policy.
use crate::{
    activity_log::{self, Entry},
    dashboard::{self, Local},
    metadata::Vocabulary,
};
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const ROW_OPACITY: [u8; 3] = [20, 40, 70];
const ROLL_DURATION: Duration = Duration::from_millis(160);

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub text: String,
    pub timestamp_unix_ms: u64,
    pub count: usize,
}
impl Group {
    pub fn label(&self, now_ms: u64) -> String {
        let minutes = now_ms.saturating_sub(self.timestamp_unix_ms) / 60_000;
        let time = if minutes == 0 {
            "just now".into()
        } else if minutes < 60 {
            format!("{minutes} min ago")
        } else if minutes.is_multiple_of(60) {
            format!("{}h ago", minutes / 60)
        } else {
            format!("{}h {}m ago", minutes / 60, minutes % 60)
        };
        if self.count > 1 {
            format!("{time} · ×{}", self.count)
        } else {
            time
        }
    }
}

fn same_day(a: Local, b: Local) -> bool {
    (a.year, a.month, a.day) == (b.year, b.month, b.day)
}

pub fn today_groups(
    mut entries: Vec<Entry>,
    now_ms: u64,
    to_local: impl Fn(u64) -> Local,
) -> Vec<Group> {
    let today = to_local(now_ms);
    entries.retain(|entry| {
        entry.timestamp_unix_ms <= now_ms && same_day(to_local(entry.timestamp_unix_ms), today)
    });
    // Stable sort retains file order for moments with the same timestamp.
    entries.sort_by_key(|entry| entry.timestamp_unix_ms);
    let mut groups: Vec<Group> = Vec::new();
    for entry in entries {
        if let Some(last) = groups.last_mut()
            && last.text == entry.text
        {
            last.count += 1;
            last.timestamp_unix_ms = entry.timestamp_unix_ms;
        } else {
            groups.push(Group {
                text: entry.text,
                timestamp_unix_ms: entry.timestamp_unix_ms,
                count: 1,
            });
        }
    }
    groups.reverse();
    groups
}

#[derive(Debug, Default)]
pub struct Recall {
    pub groups: Vec<Group>,
    pub vocabulary: Vocabulary,
    selected: Option<usize>,
    transition: Option<(f32, Duration)>,
}
impl Recall {
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn selected_text(&self) -> Option<&str> {
        self.selected.map(|index| self.groups[index].text.as_str())
    }
    pub fn older(&mut self, now: Duration) -> bool {
        if self.groups.is_empty() {
            return false;
        }
        let next = self
            .selected
            .map_or(0, |index| (index + 1).min(self.groups.len() - 1));
        self.select(Some(next), now);
        true
    }
    pub fn newer(&mut self, now: Duration) {
        let next = self.selected.and_then(|index| index.checked_sub(1));
        self.select(next, now);
    }
    fn select(&mut self, selected: Option<usize>, now: Duration) {
        if selected == self.selected {
            return;
        }
        let from = self.window(now);
        self.selected = selected;
        self.transition = Some((from, now));
    }
    pub fn clear(&mut self) {
        self.selected = None;
        self.transition = None;
    }
    /// Fractional newest visible index, shared by every monitor's animation.
    pub fn window(&self, now: Duration) -> f32 {
        let target = self.selected.unwrap_or(0) as f32;
        let Some((from, at)) = self.transition else {
            return target;
        };
        let fraction =
            (now.saturating_sub(at).as_secs_f32() / ROLL_DURATION.as_secs_f32()).min(1.0);
        let eased = fraction * fraction * (3.0 - 2.0 * fraction);
        from + (target - from) * eased
    }
    pub fn animating(&self, now: Duration) -> bool {
        self.transition
            .is_some_and(|(_, at)| now.saturating_sub(at) < ROLL_DURATION)
    }
}

pub(crate) fn system_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Loading is optional and never blocks keyboard input, guards, or dismissal.
pub struct PendingHistory {
    result: Receiver<Result<Recall, String>>,
}
impl PendingHistory {
    pub fn start() -> Result<Self, String> {
        let now_ms = system_ms();
        Self::start_with(move || {
            let directory = activity_log::directory()?;
            let today = dashboard::to_local(now_ms);
            let mut vocabulary = Vocabulary::default();
            let (entries, _) = activity_log::load_lenient_matching(&directory, |entry| {
                if entry.timestamp_unix_ms <= now_ms {
                    vocabulary.include(entry.metadata());
                }
                entry.timestamp_unix_ms <= now_ms
                    && same_day(dashboard::to_local(entry.timestamp_unix_ms), today)
            })?;
            Ok(Recall {
                groups: today_groups(entries, now_ms, dashboard::to_local),
                vocabulary,
                ..Recall::default()
            })
        })
    }
    fn start_with(
        read: impl FnOnce() -> Result<Recall, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (tx, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("activity-recall".into())
            .spawn(move || {
                let _ = tx.send(read());
            })
            .map_err(|_| "Cannot load previous reflections.".to_owned())?;
        Ok(Self { result })
    }
    pub fn poll(&self) -> Option<Result<Recall, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("Previous reflections could not be loaded.".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(text: &str, ms: u64) -> Entry {
        Entry {
            version: 1,
            metadata: None,
            id: String::new(),
            text: text.into(),
            timestamp_unix_ms: ms,
        }
    }
    fn local(ms: u64) -> Local {
        Local {
            year: 2026,
            month: 10,
            day: (ms / 86_400_000 + 1) as u32,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }
    #[test]
    fn only_today_in_local_zone_grouped_consecutively_and_sorted() {
        let day = 86_400_000;
        let entries = vec![
            entry("A", day + 3),
            entry("Old", day - 1),
            entry("A", day + 1),
            entry("B", day + 2),
            entry("A", day + 4),
            entry("Future", day + 6),
        ];
        let groups = today_groups(entries, day + 5, local);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.text.as_str(), g.count, g.timestamp_unix_ms))
                .collect::<Vec<_>>(),
            [("A", 2, day + 4), ("B", 1, day + 2), ("A", 1, day + 1)]
        );
        // A local midnight can differ from UTC midnight. Never use UTC-day math.
        let groups = today_groups(
            vec![
                entry("Local yesterday", day - 3_600_001),
                entry("Local today", day - 3_600_000),
            ],
            day,
            |ms| local(ms + 3_600_000),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].text, "Local today");
    }
    #[test]
    fn dial_rolls_clamps_and_reverses_without_wrapping() {
        let mut dial = Recall {
            groups: (0..6)
                .map(|i| Group {
                    text: i.to_string(),
                    timestamp_unix_ms: i,
                    count: 1,
                })
                .collect(),
            ..Recall::default()
        };
        assert!(dial.older(Duration::ZERO));
        assert_eq!(dial.selected(), Some(0));
        dial.older(Duration::ZERO);
        assert_eq!(dial.window(Duration::from_millis(80)), 0.5);
        assert_eq!(dial.window(Duration::from_millis(160)), 1.0);
        for _ in 0..10 {
            dial.older(Duration::from_secs(1));
        }
        assert_eq!(dial.selected(), Some(5));
        for _ in 0..10 {
            dial.newer(Duration::from_secs(2));
        }
        assert_eq!(dial.selected(), None);
        dial.clear();
        assert_eq!(dial.window(Duration::from_secs(2)), 0.0);
        assert!(!Recall::default().older(Duration::ZERO));
    }
    #[test]
    fn stalled_history_worker_never_blocks_polling_and_reports_failure() {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (go_tx, go_rx) = mpsc::sync_channel(1);
        let pending = PendingHistory::start_with(move || {
            ready_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            Err("History unavailable".into())
        })
        .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(pending.poll().is_none());
        go_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = pending.poll() {
                assert_eq!(result.unwrap_err(), "History unavailable");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn labels_use_latest_occurrence_and_exact_approved_opacities() {
        let group = Group {
            text: "Work".into(),
            timestamp_unix_ms: 60_000,
            count: 3,
        };
        assert_eq!(group.label(360_000), "5 min ago · ×3");
        assert_eq!(group.label(3_960_000), "1h 5m ago · ×3");
        assert_eq!(group.label(0), "just now · ×3");
        assert_eq!(ROW_OPACITY, [20, 40, 70]);
    }
}
