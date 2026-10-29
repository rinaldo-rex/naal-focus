//! Self-contained local activity dashboard: pure generation from history
//! entries plus a private writer and a detached browser launch. All
//! statistics derive only from documented history fields (text, timestamp);
//! untrusted text is embedded as escaped JSON, never as HTML or script.
use crate::{
    activity_log::{self, Entry},
    pending::Pending,
    private_fs,
    time::{Local, to_local, unix_millis},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::Permissions,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const TEMPLATE: &str = include_str!("dashboard_page.html");
const DATA_PLACEHOLDER: &str = "__NAAL_DATA__";
/// Beside the history, with the same privacy and link refusals.
const DASHBOARD_FILE: &str = "dashboard.html";
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Days since 1970-01-01 from a civil date (Howard Hinnant's public-domain
/// algorithms). Pure date arithmetic: no time zones, no wall clock.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = (if m <= 2 { y - 1 } else { y }) as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}
fn key_days(key: &str) -> i64 {
    let mut parts = key.split('-');
    let y: i32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let m: u32 = parts.next().unwrap_or("1").parse().unwrap_or(1);
    let d: u32 = parts.next().unwrap_or("1").parse().unwrap_or(1);
    days_from_civil(y, m, d)
}
fn key_from_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}
/// Monday that starts the week containing the given day.
fn week_start(day: i64) -> i64 {
    day - (day + 3).rem_euclid(7)
}
fn clock_12(hour: u32, minute: u32) -> String {
    format!(
        "{}:{:02} {}",
        (hour + 11) % 12 + 1,
        minute,
        if hour < 12 { "AM" } else { "PM" }
    )
}

#[derive(Serialize)]
struct Payload {
    v: u32,
    generated_ms: u64,
    generated_local: GeneratedLocal,
    today: String,
    empty: bool,
    skipped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    entries: Vec<EntryOut>,
    by_day: BTreeMap<String, u32>,
    year: i32,
    year_dates: Vec<String>,
    weeks: Vec<WeekOut>,
    tag_totals: BTreeMap<String, u32>,
    streak: StreakOut,
}
#[derive(Serialize)]
struct GeneratedLocal {
    date: String,
    date_long: String,
    time: String,
    minutes: u32,
}
#[derive(Serialize)]
struct EntryOut {
    ms: u64,
    date: String,
    minutes: u32,
    text: String,
    tags: Vec<String>,
    projects: Vec<String>,
    feelings: Vec<String>,
}
#[derive(Serialize)]
struct WeekOut {
    start: String,
    days: u32,
    total: u32,
    tags: BTreeMap<String, u32>,
}
#[derive(Serialize)]
struct StreakOut {
    current: u32,
    longest: u32,
    logged_days: u32,
    window_days: u32,
    weeks: Vec<StreakWeek>,
}
#[derive(Serialize)]
struct StreakWeek {
    start: String,
    days: u32,
    total: u32,
}

/// Build the complete self-contained page for a scanned history. Pure:
/// entries in, one HTML string out; no surfaces, disk, or wall clock.
pub fn generate(
    entries: &[Entry],
    skipped: usize,
    now_ms: u64,
    to_local: impl Fn(u64) -> Local,
) -> String {
    let now = to_local(now_ms);
    let today_days = days_from_civil(now.year, now.month, now.day);
    let today_key = now.date_key();

    let mut outs: Vec<EntryOut> = entries
        .iter()
        .map(|e| {
            let l = to_local(e.timestamp_unix_ms);
            let metadata = e.metadata();
            EntryOut {
                ms: e.timestamp_unix_ms,
                date: l.date_key(),
                minutes: l.hour * 60 + l.minute,
                text: e.text.clone(),
                tags: metadata.tags,
                projects: metadata.projects,
                feelings: metadata.feelings,
            }
        })
        .collect();
    outs.sort_by_key(|e| e.ms);

    // Reflection counts per day, indexed both by date key and by day number.
    let mut by_day: BTreeMap<String, u32> = BTreeMap::new();
    let mut counts: BTreeMap<i64, u32> = BTreeMap::new();
    for e in &outs {
        *by_day.entry(e.date.clone()).or_default() += 1;
        *counts.entry(key_days(&e.date)).or_default() += 1;
    }

    // The year map covers the current calendar year; days after today stay
    // blank cells, never zero counts.
    let first_day = days_from_civil(now.year, 1, 1);
    let last_day = days_from_civil(now.year, 12, 31);
    let year_dates: Vec<String> = (first_day..=last_day).map(key_from_days).collect();

    // Attention share: Monday-start weeks overlapping the year, through the
    // current week. Weeks without tagged reflections are omitted, never
    // inferred from totals.
    let mut tag_totals: BTreeMap<String, u32> = BTreeMap::new();
    #[derive(Default)]
    struct WeekAcc {
        total: u32,
        days: HashSet<i64>,
        tags: BTreeMap<String, u32>,
    }
    let mut weeks_acc: BTreeMap<i64, WeekAcc> = BTreeMap::new();
    let share_start = week_start(first_day);
    let current_week = week_start(today_days);
    for e in &outs {
        for tag in &e.tags {
            *tag_totals.entry(tag.clone()).or_default() += 1;
        }
        let day = key_days(&e.date);
        let ws = week_start(day);
        if (share_start..=current_week).contains(&ws) {
            let acc = weeks_acc.entry(ws).or_default();
            acc.total += 1;
            acc.days.insert(day);
            for tag in &e.tags {
                *acc.tags.entry(tag.clone()).or_default() += 1;
            }
        }
    }
    let weeks: Vec<WeekOut> = weeks_acc
        .into_iter()
        .filter(|(_, acc)| !acc.tags.is_empty())
        .map(|(ws, acc)| WeekOut {
            start: key_from_days(ws),
            days: acc.days.len() as u32,
            total: acc.total,
            tags: acc.tags,
        })
        .collect();

    // Streak: day presence only. A run ends today, or yesterday when today
    // is still quiet; a quiet day is a normal day, never a failure.
    let logged: BTreeSet<i64> = counts.keys().copied().collect();
    let mut longest = 0u32;
    let mut run = 0u32;
    let mut previous: Option<i64> = None;
    for &day in &logged {
        run = if previous == Some(day - 1) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        previous = Some(day);
    }
    let mut current = 0u32;
    let mut day = today_days;
    if !counts.contains_key(&day) {
        day -= 1;
    }
    while counts.contains_key(&day) {
        current += 1;
        day -= 1;
    }
    let window_days = 365u32;
    let logged_days = logged.range(today_days - 364..=today_days).count() as u32;
    let streak_weeks: Vec<StreakWeek> = (0..6)
        .rev()
        .map(|back| {
            let ws = current_week - 7 * back;
            let days = logged.range(ws..=ws + 6).count() as u32;
            let total: u32 = counts.range(ws..=ws + 6).map(|(_, n)| *n).sum();
            StreakWeek {
                start: key_from_days(ws),
                days,
                total,
            }
        })
        .collect();

    let payload = Payload {
        v: 1,
        generated_ms: now_ms,
        generated_local: GeneratedLocal {
            date: today_key.clone(),
            date_long: format!(
                "{} {}, {}",
                MONTHS[now.month as usize - 1],
                now.day,
                now.year
            ),
            time: clock_12(now.hour, now.minute),
            minutes: now.hour * 60 + now.minute,
        },
        today: today_key,
        empty: entries.is_empty(),
        skipped,
        error: None,
        entries: outs,
        by_day,
        year: now.year,
        year_dates,
        weeks,
        tag_totals,
        streak: StreakOut {
            current,
            longest,
            logged_days,
            window_days,
            weeks: streak_weeks,
        },
    };
    render(&payload)
}

/// A restrained error page: an unreadable history is reported as such, never
/// rendered as an empty or crashing page.
pub fn generate_error(reason: &str, now_ms: u64, to_local: impl Fn(u64) -> Local) -> String {
    let now = to_local(now_ms);
    let payload = Payload {
        v: 1,
        generated_ms: now_ms,
        generated_local: GeneratedLocal {
            date: now.date_key(),
            date_long: format!(
                "{} {}, {}",
                MONTHS[now.month as usize - 1],
                now.day,
                now.year
            ),
            time: clock_12(now.hour, now.minute),
            minutes: now.hour * 60 + now.minute,
        },
        today: now.date_key(),
        empty: true,
        skipped: 0,
        error: Some(reason.to_owned()),
        entries: Vec::new(),
        by_day: BTreeMap::new(),
        year: now.year,
        year_dates: Vec::new(),
        weeks: Vec::new(),
        tag_totals: BTreeMap::new(),
        streak: StreakOut {
            current: 0,
            longest: 0,
            logged_days: 0,
            window_days: 365,
            weeks: Vec::new(),
        },
    };
    render(&payload)
}

/// Generation entry points using the real clock and session time zone.
pub fn generate_system(entries: &[Entry], skipped: usize) -> String {
    let now_ms = unix_millis().unwrap_or_default() as u64;
    generate(entries, skipped, now_ms, to_local)
}
pub fn generate_error_system(reason: &str) -> String {
    let now_ms = unix_millis().unwrap_or_default() as u64;
    generate_error(reason, now_ms, to_local)
}

fn render(payload: &Payload) -> String {
    let json = serde_json::to_string(payload).expect("serializable payload");
    // The only placeholder is inside the inert data element; every byte of
    // untrusted text stays inside escaped JSON string values.
    TEMPLATE.replace(DATA_PLACEHOLDER, &escape_for_script(&json))
}
/// JSON bound for a `<script>` block: `<`, `&`, and the JS line separators
/// become JSON escapes, so untrusted text can never close the data element
/// or turn into executable markup.
fn escape_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Write the page privately beside the history: created or owned by the
/// current user, mode 0600, refusing symlinks, hardlinks, and foreign owners
/// exactly like `activity.jsonl`. Replaces any earlier generation in place.
fn write_private(directory: &Path, html: &str) -> Result<(), String> {
    let dir = private_fs::private_dir(directory)?;
    private_fs::lock(&dir)?;
    let file = private_fs::open_at(&dir, c"dashboard.html", libc::O_WRONLY | libc::O_CREAT)
        .map_err(|e| format!("Dashboard output is unwritable: {e}"))?;
    private_fs::check_owner(&file)?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("Dashboard output must be a regular, unlinked-to-other-files file.".into());
    }
    file.set_permissions(Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    private_fs::lock(&file)?;
    let mut file = file;
    file.set_len(0).map_err(|e| e.to_string())?;
    file.write_all(html.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    dir.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// The user-facing file path of the freshly written dashboard.
pub fn dashboard_path(directory: &Path) -> PathBuf {
    directory.join(DASHBOARD_FILE)
}

/// Rebuild the dashboard from the current history and open it in the default
/// browser. Blocking work stays on this worker; the applet only polls, so a
/// stalled filesystem cannot freeze its controls.
pub struct PendingOpen {
    result: Pending<()>,
}
impl PendingOpen {
    pub fn start() -> Result<Self, String> {
        Self::start_with(open_and_launch)
    }
    fn start_with(
        open: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<Self, String> {
        let result = Pending::start("dashboard-open", open)
            .map_err(|_| "Cannot start the dashboard worker; try again.".to_string())?;
        Ok(Self { result })
    }
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        self.result
            .poll_once("Dashboard worker stopped; try opening it again.")
    }
}
fn open_and_launch() -> Result<(), String> {
    let directory = private_fs::directory()?;
    let html = match activity_log::load_lenient(&directory) {
        Ok((entries, skipped)) => generate_system(&entries, skipped),
        // An unreadable history still opens a restrained error page rather
        // than a misleading empty one.
        Err(reason) => generate_error_system(&reason),
    };
    write_private(&directory, &html).map_err(|e| format!("Dashboard generation failed: {e}"))?;
    browser_command(&dashboard_path(&directory))
        .spawn()
        .map_err(|e| format!("Could not open the dashboard in a browser: {e}"))?;
    Ok(())
}
/// Open a local file with the desktop default browser, detached from the
/// applet: its streams are null so a chatty browser cannot block anything.
fn browser_command(path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending::tests::wait;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    /// Explicit #word vocabulary: alphanumeric plus `_`/`-`, case-insensitive,
    /// duplicate-free per entry. Trailing `-`/`_` is punctuation, stripped.
    fn parse_tags(text: &str) -> Vec<String> {
        crate::metadata::Metadata::parse(text).tags
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "naal-dashboard-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A deterministic fixed zone: the epoch's own offset, so tests never
    /// depend on the machine running them.
    fn utc(unix_ms: u64) -> Local {
        let days = (unix_ms / 86_400_000) as i64;
        let (year, month, day) = civil_from_days(days);
        let secs_of_day = (unix_ms % 86_400_000) / 1000;
        Local {
            year,
            month,
            day,
            hour: (secs_of_day / 3600) as u32,
            minute: (secs_of_day % 3600 / 60) as u32,
            second: (secs_of_day % 60) as u32,
        }
    }
    const fn ms(days_since_epoch: i64, hour: u32, minute: u32) -> u64 {
        ((days_since_epoch * 86_400 + hour as i64 * 3600 + minute as i64 * 60) * 1000) as u64
    }
    /// 2026-10-08 is a Thursday; 2026-01-01 starts the year under test.
    const TODAY_MS: u64 = ms(20734, 15, 4);
    fn entry(ms: u64, text: &str) -> Entry {
        Entry {
            version: 1,
            metadata: None,
            id: format!("{ms:032x}"),
            timestamp_unix_ms: ms,
            text: text.into(),
        }
    }
    fn payload_of(html: &str) -> serde_json::Value {
        const MARKER: &str = "<script type=\"application/json\" id=\"naal-data\">";
        let start = html.find(MARKER).expect("data marker") + MARKER.len();
        let end = start + html[start..].find("</script>").expect("data end");
        serde_json::from_str(&html[start..end]).expect("embedded JSON")
    }
    #[test]
    fn all_metadata_kinds_reach_the_dashboard_from_old_and_new_records() {
        let old = entry(TODAY_MS - 1, "Review #Rust @naal ??Focused");
        let mut new = Entry::submitted("Walk @life ??calm #outside").unwrap();
        new.timestamp_unix_ms = TODAY_MS;
        let payload = payload_of(&generate(&[old, new], 0, TODAY_MS, utc));
        assert_eq!(payload["entries"][0]["tags"][0], "rust");
        assert_eq!(payload["entries"][0]["projects"][0], "naal");
        assert_eq!(payload["entries"][0]["feelings"][0], "focused");
        assert_eq!(payload["entries"][1]["projects"][0], "life");
        assert_eq!(payload["entries"][1]["feelings"][0], "calm");
    }

    #[test]
    fn template_has_exactly_one_data_placeholder() {
        assert_eq!(TEMPLATE.matches(DATA_PLACEHOLDER).count(), 1);
    }

    #[test]
    fn civil_dates_round_trip_across_centuries() {
        for &(y, m, d) in &[
            (1970, 1, 1),
            (1969, 12, 31),
            (2000, 3, 1),
            (2024, 2, 29),
            (2026, 10, 8),
            (2100, 2, 28),
        ] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
    }

    #[test]
    fn weeks_start_on_monday() {
        // 2026-10-08 is a Thursday; its Monday is 2026-10-05.
        assert_eq!(
            key_from_days(week_start(key_days("2026-10-08"))),
            "2026-10-05"
        );
        assert_eq!(
            key_from_days(week_start(key_days("2026-10-11"))),
            "2026-10-05"
        );
        assert_eq!(
            key_from_days(week_start(key_days("2026-10-12"))),
            "2026-10-12"
        );
    }

    #[test]
    fn tags_are_explicit_lowercased_and_deduplicated() {
        assert_eq!(
            parse_tags("Finished #Rust chapter #rust #docs. and #deep_work-"),
            ["rust", "docs", "deep_work"]
        );
        assert_eq!(parse_tags("no tags here # #- #."), Vec::<String>::new());
        assert_eq!(parse_tags("#123 #café #a_b-c!"), ["123", "café", "a_b-c"]);
        assert_eq!(parse_tags("#tag, #tag. #TAG;"), ["tag"]);
    }

    #[test]
    fn generated_page_escapes_untrusted_text_inertly() {
        let text = r#"</script><script>alert("x")</script><!-- & ' " ` <img src=x onerror=1>"#;
        let html = generate(&[entry(1000, text)], 0, TODAY_MS, utc);
        assert!(!html.contains(DATA_PLACEHOLDER));
        assert!(html.contains(r#"\u003c/script>\u003cscript>alert(\"x\")"#));
        // Only the template's own tags remain as raw markup.
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("<img"));
        // Quotes and ampersands stay inside JSON string values.
        assert!(html.contains(r#"\u0026"#));
        let payload = payload_of(&html);
        assert_eq!(payload["entries"][0]["text"], text);
    }

    #[test]
    fn empty_history_generates_a_friendly_empty_state() {
        let html = generate(&[], 0, TODAY_MS, utc);
        assert!(html.contains("Nothing recorded yet"));
        assert!(html.contains("stays on this computer"));
        let payload = payload_of(&html);
        assert_eq!(payload["empty"], true);
        assert_eq!(payload["skipped"], 0);
        assert_eq!(payload["entries"].as_array().unwrap().len(), 0);
        assert!(payload.get("error").is_none());
    }

    #[test]
    fn unreadable_history_generates_a_restrained_error_page() {
        let html = generate_error("permission <denied> & locked", TODAY_MS, utc);
        assert!(html.contains("History could not be read"));
        let payload = payload_of(&html);
        assert_eq!(payload["error"], "permission <denied> & locked");
        assert!(html.contains(r"\u003cdenied>"));
    }

    #[test]
    fn aggregation_buckets_days_weeks_and_streaks_from_timestamps() {
        // Longest run: 2026-08-31..09-04 (Mon..Fri), all tagged #early.
        let mut entries: Vec<Entry> = (0..5)
            .map(|d| entry(ms(20696 + d, 8, 0), "#early morning"))
            .collect();
        // Current run: 2026-10-05..08, today included.
        entries.push(entry(ms(20731, 9, 0), "#rust reading"));
        entries.push(entry(ms(20732, 10, 30), "quiet #docs morning"));
        entries.push(entry(ms(20733, 11, 4), "#rust again"));
        entries.push(entry(ms(20733, 23, 59), "untagged, same day"));
        entries.push(entry(ms(20734, 15, 4), "#Rust #rust duplicate words"));
        // Same Monday week: Sunday 2026-10-11.
        entries.push(entry(ms(20737, 7, 0), "#weekend rest"));
        let html = generate(&entries, 3, TODAY_MS, utc);
        let payload = payload_of(&html);

        assert_eq!(payload["skipped"], 3);
        assert_eq!(payload["today"], "2026-10-08");
        // Day counts: two reflections on the 7th, one on each other day.
        assert_eq!(payload["by_day"]["2026-10-05"], 1);
        assert_eq!(payload["by_day"]["2026-10-06"], 1);
        assert_eq!(payload["by_day"]["2026-10-07"], 2);
        assert_eq!(payload["by_day"]["2026-10-08"], 1);
        assert_eq!(payload["by_day"]["2026-10-11"], 1);
        // Entries stay chronological; per-entry tags are parsed and deduped.
        let out = payload["entries"].as_array().unwrap();
        assert_eq!(out.len(), entries.len());
        assert!(
            out.windows(2)
                .all(|w| w[0]["ms"].as_u64() <= w[1]["ms"].as_u64())
        );
        assert_eq!(out[6]["tags"].as_array().unwrap().len(), 1);
        assert_eq!(out[6]["tags"][0], "docs");
        assert_eq!(out[9]["tags"][0], "rust");
        assert_eq!(out[9]["minutes"], 15 * 60 + 4);
        // Tag totals span the whole history.
        assert_eq!(payload["tag_totals"]["early"], 5);
        assert_eq!(payload["tag_totals"]["rust"], 3);
        assert_eq!(payload["tag_totals"]["docs"], 1);
        assert_eq!(payload["tag_totals"]["weekend"], 1);
        // Attention share: Monday weeks of the year with tagged reflections.
        let weeks = payload["weeks"].as_array().unwrap();
        assert_eq!(weeks.len(), 2);
        assert_eq!(weeks[0]["start"], "2026-08-31");
        assert_eq!(weeks[0]["tags"]["early"], 5);
        assert_eq!(weeks[0]["total"], 5);
        assert_eq!(weeks[1]["start"], "2026-10-05");
        assert_eq!(weeks[1]["tags"]["rust"], 3);
        assert_eq!(weeks[1]["tags"]["docs"], 1);
        assert_eq!(weeks[1]["total"], 6);
        // Streaks: presence only, current run ends today.
        assert_eq!(payload["streak"]["current"], 4);
        assert_eq!(payload["streak"]["longest"], 5);
        assert_eq!(payload["streak"]["logged_days"], 9);
        assert_eq!(payload["streak"]["window_days"], 365);
        let circles = payload["streak"]["weeks"].as_array().unwrap();
        assert_eq!(circles.len(), 6);
        assert_eq!(circles[5]["start"], "2026-10-05");
        assert_eq!(circles[5]["days"], 5);
        assert_eq!(circles[5]["total"], 6);
        assert_eq!(circles[0]["start"], "2026-08-31");
        // The year map covers the full calendar year, ending blank ahead.
        let year_dates = payload["year_dates"].as_array().unwrap();
        assert_eq!(year_dates.len(), 365);
        assert_eq!(year_dates[0], "2026-01-01");
        assert_eq!(year_dates[364], "2026-12-31");
    }

    #[test]
    fn writer_is_private_and_replaces_earlier_generations() {
        let temp = Temp::new();
        let dir = temp.0.join("out");
        write_private(&dir, "<!doctype html>first").unwrap();
        let path = dir.join(DASHBOARD_FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        write_private(&dir, "<!doctype html>second, longer").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "<!doctype html>second, longer"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn writer_refuses_symlinks_hardlinks_and_foreign_targets() {
        let temp = Temp::new();
        let dir = temp.0.join("out");
        fs::create_dir(&dir).unwrap();
        let target = temp.0.join("outside");
        fs::write(&target, b"untouched").unwrap();
        let path = dir.join(DASHBOARD_FILE);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(write_private(&dir, "<!doctype html>x").is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(write_private(&dir, "<!doctype html>x").is_err());
        let alias = temp.0.join("alias");
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        assert!(write_private(&alias, "<!doctype html>x").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
    }

    #[test]
    fn dashboard_worker_results_and_panics_are_reported_once_without_a_browser() {
        for result in [Ok(()), Err("Fixture dashboard failure.".into())] {
            let expected = result.clone();
            let mut pending = PendingOpen::start_with(move || {
                assert_eq!(std::thread::current().name(), Some("dashboard-open"));
                result
            })
            .unwrap();
            assert_eq!(wait(|| pending.poll()), expected);
            assert!(pending.poll().is_none());
        }
        let mut stopped = PendingOpen::start_with(|| {
            panic!("simulated dashboard worker panic");
        })
        .unwrap();
        assert_eq!(
            wait(|| stopped.poll()),
            Err("Dashboard worker stopped; try opening it again.".into())
        );
        assert!(stopped.poll().is_none());
    }

    #[test]
    fn browser_command_opens_the_local_file_detached() {
        let command = browser_command(Path::new("/home/me/.naal-focus/dashboard.html"));
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("/home/me/.naal-focus/dashboard.html")]
        );
    }
}
