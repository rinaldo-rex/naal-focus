use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub const APP_ID: &str = "io.github.rinaldo_rex.NaalFocus";
pub const DEFAULT_QUOTES: &str = include_str!("../data/quotes.txt");
// Covers the worst-case UTF-8 size of 100 × 500-character valid quotes.
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub enabled: bool,
    pub interval_secs: u64,
    pub duration_secs: u64,
    pub typing_limit_secs: u64,
    pub text_size: u32,
    pub idle_secs: u32,
    pub quotes: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            // Recurring focus capture is explicitly opt-in, even after installation.
            enabled: false,
            interval_secs: 300,
            duration_secs: 5,
            typing_limit_secs: 120,
            text_size: 36,
            idle_secs: 120,
            quotes: parse_quotes(DEFAULT_QUOTES).expect("valid bundled quotes"),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported settings version; your file has not been overwritten.".into());
        }
        if !(15..=86400).contains(&self.interval_secs) {
            return Err("Interval must be 15–86400 seconds.".into());
        }
        if !(1..=30).contains(&self.duration_secs) {
            return Err("Duration must be 1–30 seconds.".into());
        }
        if !(30..=600).contains(&self.typing_limit_secs) {
            return Err("Typing time limit must be 30–600 seconds.".into());
        }
        if self.interval_secs <= self.duration_secs {
            return Err("Interval must exceed the visible duration.".into());
        }
        if !(22..=72).contains(&self.text_size) {
            return Err("Text size must be 22–72 logical pixels.".into());
        }
        if !(30..=3600).contains(&self.idle_secs) {
            return Err("Idle threshold must be 30–3600 seconds.".into());
        }
        validate_quotes(&self.quotes)
    }
}

/// Bulk-editor syntax, not Markdown: top-level `- ` starts an item, while
/// continuation lines (including blank lines) stay in that item. Remove one
/// two-space editor indent; an indented `- ` therefore stays literal text.
/// Unbulleted one-per-line lists remain supported for legacy pastes/defaults.
pub fn parse_quotes(text: &str) -> Result<Vec<String>, String> {
    if text.len() > MAX_CONFIG_BYTES as usize {
        return Err("Quote collection is too large.".into());
    }
    let quotes = if text.lines().any(|line| quote_start(line).is_some()) {
        let mut quotes = Vec::new();
        let mut current: Option<String> = None;
        for (index, line) in text.lines().enumerate() {
            if let Some(first_line) = quote_start(line) {
                if let Some(previous) = current.replace(first_line.to_owned()) {
                    quotes.push(trim_blank_edges(&previous));
                }
            } else if let Some(quote) = &mut current {
                quote.push('\n');
                quote.push_str(line.strip_prefix("  ").unwrap_or(line));
            } else if !line.trim().is_empty() {
                return Err(format!(
                    "Line {}: text before the first quote; start it with '- '.",
                    index + 1
                ));
            }
        }
        if let Some(quote) = current {
            quotes.push(trim_blank_edges(&quote));
        }
        quotes
    } else {
        text.lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    validate_quotes(&quotes)?;
    Ok(quotes)
}

fn quote_start(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| (line == "-").then_some(""))
}

fn trim_blank_edges(quote: &str) -> String {
    let lines: Vec<_> = quote.lines().collect();
    let first = lines.iter().position(|line| !line.trim().is_empty());
    let last = lines.iter().rposition(|line| !line.trim().is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

/// Escape every continuation with two spaces so even literal bullet lines
/// round-trip as part of one quote rather than becoming additional items.
pub fn format_quotes(quotes: &[String]) -> String {
    quotes
        .iter()
        .map(|quote| format!("- {}", quote.replace('\n', "\n  ")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn validate_quotes(quotes: &[String]) -> Result<(), String> {
    if quotes.is_empty() || quotes.len() > 100 {
        return Err("Provide 1–100 quotes.".into());
    }
    for (index, quote) in quotes.iter().enumerate() {
        let number = index + 1;
        if quote.trim().is_empty() {
            return Err(format!("Quote {number} is empty."));
        }
        if quote.chars().count() > 500 {
            return Err(format!(
                "Quote {number} exceeds 500 characters (including line breaks)."
            ));
        }
        if quote.chars().any(|c| c.is_control() && c != '\n') {
            return Err(format!(
                "Quote {number}: line breaks are allowed, but tabs and other control characters are not."
            ));
        }
    }
    Ok(())
}

pub fn config_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("Neither XDG_CONFIG_HOME nor HOME is available.")?;
    Ok(base.join("naal-focus/settings.json"))
}

pub fn load(path: &Path) -> Result<Settings, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("Settings file is too large.".into());
    }
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let settings: Settings =
        serde_json::from_slice(&data).map_err(|e| format!("Invalid settings: {e}"))?;
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    let parent = path
        .parent()
        .ok_or("Settings path needs a parent directory.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".settings-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| format!("Cannot save settings: {e}"))?;
        let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_opt_in_and_valid() {
        let s = Settings::default();
        assert!(!s.enabled);
        assert_eq!(s.interval_secs, 300);
        assert_eq!(s.duration_secs, 5);
        s.validate().unwrap();
    }

    #[test]
    fn validates_boundaries_and_quotes() {
        for (interval, duration) in [(14, 5), (15, 15), (300, 0), (300, 31), (86401, 5)] {
            assert!(
                Settings {
                    interval_secs: interval,
                    duration_secs: duration,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        assert_eq!(
            parse_quotes("  hello \n\n world ").unwrap(),
            ["hello", "world"]
        );
        for bad in [
            String::new(),
            "a".repeat(501),
            "tab\tquote".into(),
            "q\n".repeat(101),
        ] {
            assert!(parse_quotes(&bad).is_err());
        }
        assert!(parse_quotes(&"é".repeat(500)).is_ok());
    }

    #[test]
    fn bullets_keep_multiline_paragraphs_and_nested_bullets_together() {
        assert_eq!(parse_quotes("\n- First line.\n  Second line.\n\n  New paragraph.\n  - Literal bullet.\n\n- Another quote.\nUnindented continuation.\n\n").unwrap(), [
            "First line.\nSecond line.\n\nNew paragraph.\n- Literal bullet.",
            "Another quote.\nUnindented continuation.",
        ]);
        assert_eq!(
            parse_quotes("- First.\r\n  Second.\r\n\r\n- Next.").unwrap(),
            ["First.\nSecond.", "Next."]
        );
        assert!(parse_quotes("A forgotten marker.\n- Next quote.").is_err());
        for empty in ["-", "- \n- Valid.", "- Valid.\n-", "- \n  \n- Valid."] {
            assert!(parse_quotes(empty).is_err(), "{empty:?}");
        }
    }

    #[test]
    fn bullet_editor_round_trips_saved_quotes_and_literal_markers() {
        let mut quotes = Settings::default().quotes;
        quotes.extend([
            "Take a breath.\n\nThen return. 🪷".into(),
            "- Literal first bullet.\n- Literal continuation.\n  - Nested bullet.".into(),
            "  Leading spaces.\n    Indented continuation.  ".into(),
        ]);
        assert_eq!(parse_quotes(&format_quotes(&quotes)).unwrap(), quotes);
    }

    #[test]
    fn multiline_quotes_keep_character_and_control_limits() {
        let mut settings = Settings {
            quotes: vec![format!("{}\n🪷", "é".repeat(498))],
            ..Settings::default()
        };
        settings.validate().unwrap();
        assert_eq!(
            parse_quotes(&format_quotes(&settings.quotes)).unwrap(),
            settings.quotes
        );
        settings.quotes[0].push('a');
        assert!(settings.validate().is_err());
        for bad in [
            "first\tsecond",
            "first\rsecond",
            "first\u{1b}second",
            "first\0second",
            "\n  \n",
        ] {
            settings.quotes = vec![bad.into()];
            assert!(settings.validate().is_err(), "{bad:?}");
        }
        assert!(parse_quotes(&"- Quote.\n".repeat(101)).is_err());
        assert!(parse_quotes(&"x".repeat(MAX_CONFIG_BYTES as usize + 1)).is_err());
    }

    #[test]
    fn round_trip_and_corrupt_file_preservation() {
        let dir = std::env::temp_dir().join(format!("naal-config-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        assert_eq!(load(&path).unwrap(), Settings::default());
        save(&path, &Settings::default()).unwrap();
        assert_eq!(load(&path).unwrap(), Settings::default());
        let maximum = Settings {
            quotes: vec!["🪷".repeat(500); 100],
            ..Settings::default()
        };
        save(&path, &maximum).unwrap();
        assert_eq!(load(&path).unwrap(), maximum);
        let multiline = Settings {
            quotes: vec!["First line.\n\nAnother paragraph.\n- A literal bullet.".into()],
            ..Settings::default()
        };
        save(&path, &multiline).unwrap();
        assert_eq!(load(&path).unwrap(), multiline);
        fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not json");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typing_limit_is_bounded_and_old_settings_get_the_default() {
        let old: Settings = serde_json::from_str(r#"{"version":1,"duration_secs":5}"#).unwrap();
        assert_eq!(old.typing_limit_secs, 120);
        old.validate().unwrap();
        for limit in [0, 29, 601, u64::MAX] {
            assert!(
                Settings {
                    typing_limit_secs: limit,
                    ..Settings::default()
                }
                .validate()
                .is_err()
            );
        }
        for limit in [30, 120, 600] {
            let settings = Settings {
                typing_limit_secs: limit,
                ..Settings::default()
            };
            settings.validate().unwrap();
            assert_eq!(
                serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap())
                    .unwrap(),
                settings
            );
        }
    }

    #[test]
    fn schema_is_versioned_and_strict() {
        assert!(serde_json::from_str::<Settings>(r#"{"typo": 5}"#).is_err());
        let s: Settings = serde_json::from_str(r#"{"version": 2}"#).unwrap();
        assert!(s.validate().is_err());
    }
}
