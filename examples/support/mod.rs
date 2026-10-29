use naal_focus::activity_log::Entry;
use std::{error::Error, fs, path::PathBuf};

pub const NOW_MS: u64 = 1_793_271_600_000;

pub fn entry(index: usize, timestamp_unix_ms: u64, text: &str) -> Entry {
    Entry {
        version: 1,
        metadata: None,
        id: format!("{index:032x}"),
        timestamp_unix_ms,
        text: text.into(),
    }
}

pub fn entries() -> Vec<Entry> {
    [
        "Read #rust @demo ??focused",
        "Walk #outside @life ??calm",
        "Review #rust @demo ??curious",
    ]
    .into_iter()
    .enumerate()
    .map(|(i, text)| entry(i, NOW_MS - (3 - i as u64) * 3_600_000, text))
    .collect()
}

pub fn output_directory() -> Result<PathBuf, Box<dyn Error>> {
    // Rendered clock labels use the session zone; fix it before this process starts.
    if std::env::var("TZ").as_deref() != Ok("UTC0") {
        return Err("Run this demo with TZ=UTC0 for reproducible clock labels.".into());
    }
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().ok_or("Supply an output directory.")?);
    if args.next().is_some() {
        return Err("Supply exactly one output directory.".into());
    }
    fs::create_dir_all(&directory)?;
    Ok(directory)
}
