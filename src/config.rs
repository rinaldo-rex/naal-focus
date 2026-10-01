use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub const APP_ID: &str = "io.github.rinaldo_rex.NaalFocus";
pub const DEFAULT_QUOTES: &str = include_str!("../data/quotes.txt");
pub const MAX_CONFIG_BYTES: u64 = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub enabled: bool,
    pub interval_secs: u64,
    pub duration_secs: u64,
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

pub fn parse_quotes(text: &str) -> Result<Vec<String>, String> {
    let quotes: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    validate_quotes(&quotes)?;
    Ok(quotes)
}

fn validate_quotes(quotes: &[String]) -> Result<(), String> {
    if quotes.is_empty() || quotes.len() > 100 {
        return Err("Provide 1–100 quotes, one per line.".into());
    }
    if quotes
        .iter()
        .any(|q| q.trim().is_empty() || q.chars().count() > 500 || q.chars().any(char::is_control))
    {
        return Err(
            "Each quote must be non-empty, at most 500 characters, with no control characters."
                .into(),
        );
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
    fn round_trip_and_corrupt_file_preservation() {
        let dir = std::env::temp_dir().join(format!("naal-config-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        assert_eq!(load(&path).unwrap(), Settings::default());
        save(&path, &Settings::default()).unwrap();
        assert_eq!(load(&path).unwrap(), Settings::default());
        fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not json");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schema_is_versioned_and_strict() {
        assert!(serde_json::from_str::<Settings>(r#"{"typo": 5}"#).is_err());
        let s: Settings = serde_json::from_str(r#"{"version": 2}"#).unwrap();
        assert!(s.validate().is_err());
    }
}
