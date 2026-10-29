//! Calendar time for history and display. Reminder deadlines use `process::boot_time`.
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Local {
    pub fn date(self) -> (i32, u32, u32) {
        (self.year, self.month, self.day)
    }

    pub fn date_key(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Time-zone conversion through the C library, honoring the session zone.
pub fn to_local(unix_ms: u64) -> Local {
    let secs = (unix_ms / 1000).min(i64::MAX as u64) as libc::time_t;
    // SAFETY: tm is a live writable struct; localtime_r only writes through
    // the given pointer and is thread-safe, as is the gmtime_r fallback.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        // SAFETY: g is a live writable struct; gmtime_r writes only to it.
        let mut g: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::gmtime_r(&secs, &mut g) }.is_null() {
            return Local {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
            };
        }
        return from_tm(g);
    }
    from_tm(tm)
}

fn from_tm(tm: libc::tm) -> Local {
    Local {
        year: tm.tm_year.saturating_add(1900),
        month: tm.tm_mon.max(0) as u32 + 1,
        day: tm.tm_mday.max(1) as u32,
        hour: tm.tm_hour.max(0) as u32,
        minute: tm.tm_min.max(0) as u32,
        second: tm.tm_sec.max(0) as u32,
    }
}

// Keep precision so persistence and display retain their own error/overflow policies.
pub(crate) fn unix_millis() -> Result<u128, &'static str> {
    millis_since_epoch(SystemTime::now())
}

fn millis_since_epoch(at: SystemTime) -> Result<u128, &'static str> {
    at.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| "System clock is before the Unix epoch.")
}

pub(crate) fn system_ms() -> u64 {
    unix_millis().unwrap_or_default().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, time::Duration};

    #[test]
    fn local_date_identity_and_keys_ignore_the_time_of_day() {
        let midnight = Local {
            year: 2024,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        let evening = Local {
            hour: 23,
            minute: 59,
            second: 59,
            ..midnight
        };
        assert_eq!(midnight.date(), (2024, 2, 29));
        assert_eq!(midnight.date(), evening.date());
        assert_eq!(midnight.date_key(), "2024-02-29");
        assert_eq!(midnight.date_key(), evening.date_key());
        let next_day = Local {
            month: 3,
            day: 1,
            ..midnight
        };
        assert_ne!(midnight.date(), next_day.date());
        assert_eq!(next_day.date_key(), "2024-03-01");
    }

    #[test]
    fn timestamps_preserve_precision_range_and_pre_epoch_errors() {
        assert_eq!(millis_since_epoch(UNIX_EPOCH), Ok(0));
        assert_eq!(
            millis_since_epoch(UNIX_EPOCH + Duration::from_micros(1_234_999)),
            Ok(1234)
        );
        assert_eq!(
            millis_since_epoch(UNIX_EPOCH - Duration::from_nanos(1)),
            Err("System clock is before the Unix epoch.")
        );
        let beyond_u64 = UNIX_EPOCH + Duration::from_millis(u64::MAX) + Duration::from_millis(1);
        assert_eq!(millis_since_epoch(beyond_u64), Ok(u64::MAX as u128 + 1));
    }

    #[test]
    fn c_calendar_fields_keep_the_existing_normalization() {
        // SAFETY: every field of libc::tm accepts a zero value.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = 124;
        tm.tm_mon = 1;
        tm.tm_mday = 29;
        tm.tm_hour = 23;
        tm.tm_min = 59;
        tm.tm_sec = 60;
        assert_eq!(
            from_tm(tm),
            Local {
                year: 2024,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 60,
            }
        );
        tm.tm_year = i32::MAX;
        tm.tm_mon = -1;
        tm.tm_mday = 0;
        tm.tm_hour = -1;
        tm.tm_min = -1;
        tm.tm_sec = -1;
        assert_eq!(from_tm(tm).year, i32::MAX);
        assert_eq!(from_tm(tm).date_key(), "2147483647-01-01");
        assert_eq!(from_tm(tm).hour, 0);
        assert_eq!(from_tm(tm).minute, 0);
        assert_eq!(from_tm(tm).second, 0);
    }

    #[test]
    fn local_conversion_uses_session_timezone_and_dst() {
        const TEST_ZONE: &str = "NAAL_FOCUS_TIME_TEST_ZONE";
        const ZONES: [&str; 3] = ["UTC0", "IST-5:30", "EST5EDT,M3.2.0/2,M11.1.0/2"];
        if let Ok(zone) = std::env::var(TEST_ZONE) {
            type Calendar = (i32, u32, u32, u32, u32, u32);
            let cases: &[(u64, Calendar)] = match zone.as_str() {
                "UTC0" => &[
                    (0, (1970, 1, 1, 0, 0, 0)),
                    (1_709_164_800_999, (2024, 2, 29, 0, 0, 0)),
                ],
                "IST-5:30" => &[
                    (0, (1970, 1, 1, 5, 30, 0)),
                    (1_709_145_000_000, (2024, 2, 29, 0, 0, 0)),
                ],
                "EST5EDT,M3.2.0/2,M11.1.0/2" => &[
                    (0, (1969, 12, 31, 19, 0, 0)),
                    (1_710_053_999_000, (2024, 3, 10, 1, 59, 59)),
                    (1_710_054_000_000, (2024, 3, 10, 3, 0, 0)),
                    (1_730_613_599_000, (2024, 11, 3, 1, 59, 59)),
                    (1_730_613_600_000, (2024, 11, 3, 1, 0, 0)),
                ],
                _ => panic!("Unknown test zone: {zone}"),
            };
            for &(timestamp, expected) in cases {
                let local = to_local(timestamp);
                assert_eq!(
                    (
                        local.year,
                        local.month,
                        local.day,
                        local.hour,
                        local.minute,
                        local.second
                    ),
                    expected,
                    "{zone} at {timestamp}"
                );
            }
            return;
        }

        // TZ is process-global; isolate each fixture rather than mutate parallel tests.
        for zone in ZONES {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "time::tests::local_conversion_uses_session_timezone_and_dst",
                ])
                .env("TZ", zone)
                .env(TEST_ZONE, zone)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{zone}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
