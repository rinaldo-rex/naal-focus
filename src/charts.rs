//! Current-local-day mention counts. No inferred durations or draft metadata.
use crate::{
    activity_log::Entry,
    dashboard::Local,
    metadata::{Kind, Metadata},
};
use std::collections::{BTreeMap, HashMap};

type Day = (i32, u32, u32);
fn day(local: Local) -> Day {
    (local.year, local.month, local.day)
}
fn index(kind: Kind) -> usize {
    match kind {
        Kind::Project => 0,
        Kind::Feeling => 1,
        Kind::Tag => 2,
    }
}
fn values(metadata: &Metadata, kind: Kind) -> &[String] {
    match kind {
        Kind::Project => &metadata.projects,
        Kind::Feeling => &metadata.feelings,
        Kind::Tag => &metadata.tags,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    pub label: String,
    pub hours: [u64; 24],
}
impl Series {
    pub fn total(&self) -> u64 {
        self.hours.iter().copied().sum()
    }
}

#[derive(Debug)]
struct Observation {
    hour: usize,
    metadata: Metadata,
}

#[derive(Debug, Default)]
pub struct DailyCharts {
    date: Option<Day>,
    pub hour: u32,
    counts: [BTreeMap<String, [u64; 24]>; 3],
    // Only today's compact metadata is retained, allowing snapshot/save races
    // and same-ID corrections to replace counts rather than double-count them.
    records: HashMap<String, Observation>,
}
impl DailyCharts {
    pub fn from_entries(entries: &[Entry], now_ms: u64, to_local: impl Fn(u64) -> Local) -> Self {
        let mut charts = Self::default();
        charts.advance(to_local(now_ms));
        for entry in entries {
            charts.upsert(entry, now_ms, &to_local);
        }
        charts
    }

    /// Hour/day rollover never carries yesterday's counts into a new day.
    pub fn advance(&mut self, now: Local) -> bool {
        let changed = self.date != Some(day(now)) || self.hour != now.hour;
        if self.date != Some(day(now)) {
            *self = Self {
                date: Some(day(now)),
                ..Self::default()
            };
        }
        self.hour = now.hour.min(23);
        changed
    }

    pub fn upsert(&mut self, entry: &Entry, now_ms: u64, to_local: impl Fn(u64) -> Local) {
        self.advance(to_local(now_ms));
        if entry.timestamp_unix_ms > now_ms {
            return;
        }
        let local = to_local(entry.timestamp_unix_ms);
        if self.date != Some(day(local)) {
            return;
        }
        if !entry.id.is_empty()
            && let Some(previous) = self.records.remove(&entry.id)
        {
            self.apply(&previous, false);
        }
        let observation = Observation {
            hour: local.hour.min(23) as usize,
            metadata: entry.metadata(),
        };
        self.apply(&observation, true);
        if !entry.id.is_empty() {
            self.records.insert(entry.id.clone(), observation);
        }
    }

    fn apply(&mut self, observation: &Observation, add: bool) {
        for kind in [Kind::Project, Kind::Feeling, Kind::Tag] {
            let counts = &mut self.counts[index(kind)];
            for value in values(&observation.metadata, kind) {
                let hours = counts.entry(value.clone()).or_default();
                if add {
                    hours[observation.hour] += 1;
                } else {
                    hours[observation.hour] = hours[observation.hour].saturating_sub(1);
                }
                if hours.iter().all(|&count| count == 0) {
                    counts.remove(value);
                }
            }
        }
    }

    /// A bounded ranked view; overflow is combined, never silently discarded.
    /// Each row counts a value once per reflection; Other sums value mentions.
    pub fn rows(&self, kind: Kind, limit: usize) -> Vec<Series> {
        if limit == 0 {
            return Vec::new();
        }
        let mut rows: Vec<_> = self.counts[index(kind)]
            .iter()
            .map(|(value, hours)| Series {
                label: format!("{}{value}", kind.prefix()),
                hours: *hours,
            })
            .collect();
        rows.sort_by(|a, b| b.total().cmp(&a.total()).then(a.label.cmp(&b.label)));
        if rows.len() > limit {
            let overflow = rows.split_off(limit - 1);
            let mut other = Series {
                label: format!("Other ({})", overflow.len()),
                hours: [0; 24],
            };
            for row in overflow {
                for (total, count) in other.hours.iter_mut().zip(row.hours) {
                    *total += count;
                }
            }
            rows.push(other);
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local(ms: u64) -> Local {
        Local {
            year: 2026,
            month: 10,
            day: (ms / 86_400_000 + 1) as u32,
            hour: (ms / 3_600_000 % 24) as u32,
            minute: 0,
            second: 0,
        }
    }
    fn entry(id: &str, text: &str, timestamp_unix_ms: u64) -> Entry {
        Entry {
            version: 1,
            id: id.into(),
            text: text.into(),
            timestamp_unix_ms,
            metadata: None,
        }
    }
    #[test]
    fn counts_local_today_once_per_value_excluding_old_and_future() {
        let entries = [
            entry("old", "#old", 1),
            entry("a", "#Rust #rust @naal ??calm", 90_000_000),
            entry("b", "#rust #reading @naal", 94_000_000),
            entry("future", "#future", 100_000_000),
        ];
        let charts = DailyCharts::from_entries(&entries, 95_000_000, local);
        let tags = charts.rows(Kind::Tag, 4);
        assert_eq!(
            tags.iter()
                .map(|s| (s.label.as_str(), s.total()))
                .collect::<Vec<_>>(),
            [("#rust", 2), ("#reading", 1)]
        );
        assert_eq!(tags[0].hours[1], 1);
        assert_eq!(tags[0].hours[2], 1);
        assert_eq!(charts.rows(Kind::Project, 4)[0].total(), 2);
        assert_eq!(charts.rows(Kind::Feeling, 4)[0].total(), 1);
        // Session-local midnight, not UTC midnight.
        let shifted =
            DailyCharts::from_entries(&[entry("a", "#yes", 86_000_000)], 87_000_000, |ms| {
                local(ms + 3_600_000)
            });
        assert_eq!(shifted.rows(Kind::Tag, 4)[0].label, "#yes");
    }
    #[test]
    fn acknowledgment_races_retries_and_corrections_replace_same_record() {
        let original = entry("same", "#rust @naal ??calm", 3_600_000);
        let mut charts =
            DailyCharts::from_entries(std::slice::from_ref(&original), 8_000_000, local);
        charts.upsert(&original, 8_000_000, local);
        assert_eq!(charts.rows(Kind::Tag, 4)[0].total(), 1);
        charts.upsert(
            &entry("same", "#reading ??focused", original.timestamp_unix_ms),
            8_000_000,
            local,
        );
        assert!(charts.rows(Kind::Project, 4).is_empty());
        assert_eq!(charts.rows(Kind::Tag, 4)[0].label, "#reading");
        assert_eq!(charts.rows(Kind::Feeling, 4)[0].label, "??focused");
        charts.upsert(
            &entry("same", "No metadata", original.timestamp_unix_ms),
            8_000_000,
            local,
        );
        assert!(charts.rows(Kind::Tag, 4).is_empty());
    }
    #[test]
    fn overflow_preserves_mentions_and_legacy_entries_are_not_merged() {
        let entries = [entry("", "#a #b #c #d #e", 1), entry("", "#a #b", 2)];
        let mut charts = DailyCharts::from_entries(&entries, 3, local);
        let rows = charts.rows(Kind::Tag, 4);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].label, "#a");
        assert_eq!(rows[3].label, "Other (2)");
        assert_eq!(rows.iter().map(Series::total).sum::<u64>(), 7);
        assert!(charts.rows(Kind::Tag, 0).is_empty());
        assert!(charts.advance(local(86_400_000)));
        assert!(charts.rows(Kind::Tag, 4).is_empty());
        assert!(!charts.advance(local(86_400_001)));
        assert!(charts.advance(local(90_000_000)));
    }
}
