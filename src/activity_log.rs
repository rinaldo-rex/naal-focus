//! Private, append-only activity history. Only explicitly submitted text is stored.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_ACTIVITY_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u32,
    /// UTC milliseconds since the Unix epoch, captured on submission.
    pub timestamp_unix_ms: u64,
    pub text: String,
}

impl Entry {
    pub fn submitted(text: &str) -> Result<Self, String> {
        validate_text(text)?;
        let timestamp_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "System clock is before the Unix epoch.")?
            .as_millis()
            .try_into()
            .map_err(|_| "System timestamp is out of range.")?;
        Ok(Self {
            version: 1,
            timestamp_unix_ms,
            text: text.to_owned(),
        })
    }
}

pub fn validate_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Write something before saving.".into());
    }
    if text.chars().count() > MAX_ACTIVITY_CHARS {
        return Err(format!(
            "Activity text must be at most {MAX_ACTIVITY_CHARS} characters."
        ));
    }
    if text.chars().any(char::is_control) {
        return Err("Activity text must be a single line without control characters.".into());
    }
    Ok(())
}

pub fn directory() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(".naal-focus"))
        .ok_or_else(|| "An absolute HOME is required to save activity logs.".into())
}

pub fn save(entry: &Entry) -> Result<(), String> {
    append_in(&directory()?, entry).map_err(|e| format!("Activity was not saved: {e}"))
}

/// Nonblocking file locking keeps concurrent writes out of one another's records.
/// The caller must run this on a worker: filesystem I/O can stall independently
/// of the overlay, whose parent watchdog must remain active.
fn append_in(directory: &Path, entry: &Entry) -> Result<(), String> {
    validate_text(&entry.text)?;
    if entry.version != 1 {
        return Err("Unsupported activity log version.".into());
    }
    let mut bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    // Open the directory itself without following a symlink, then use openat
    // so a renamed/replaced path cannot redirect this save elsewhere.
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)
        .map_err(|e| e.to_string())?;
    check_owner(&dir)?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    // SAFETY: dir is live, the filename is NUL-terminated, and mode is supplied
    // because O_CREAT is set. Ownership of the returned fd is transferred once.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c"activity.jsonl".as_ptr(),
            libc::O_RDWR
                | libc::O_CREAT
                | libc::O_APPEND
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: openat returned a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    check_owner(&file)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("Activity history must be a regular, unlinked-to-other-files file.".into());
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    // SAFETY: file is live; flock has no pointer arguments.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Activity history is busy; try again.".into());
    }
    let original_len = file.metadata().map_err(|e| e.to_string())?.len();
    if original_len > 0 {
        file.seek(SeekFrom::End(-1)).map_err(|e| e.to_string())?;
        let mut tail = [0];
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        if tail != [b'\n'] {
            return Err("Activity history has an incomplete last record; repair it before retrying. Existing data was preserved.".into());
        }
    }
    // Sync the directory entry before committing a record; acknowledgment comes
    // only after the entire appended line has been synced. A reported append
    // failure rolls back our own bytes under the lock, never previous records.
    dir.sync_all().map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        if file
            .set_len(original_len)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            return Err("Activity history needs inspection after an I/O error; save outcome is uncertain. Do not blindly resubmit.".into());
        }
        return Err(error.to_string());
    }
    Ok(())
}

fn check_owner(file: &File) -> Result<(), String> {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    if file.metadata().map_err(|e| e.to_string())?.uid() != unsafe { libc::geteuid() } {
        return Err("Activity history must be owned by the current user.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "naal-activity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn entry(text: &str) -> Entry {
        Entry {
            version: 1,
            timestamp_unix_ms: 1234,
            text: text.into(),
        }
    }

    #[test]
    fn validates_single_line_utf8_and_timestamp() {
        for text in ["", "  ", "a\nb", "a\tb", "a\0b"] {
            assert!(Entry::submitted(text).is_err());
        }
        assert!(Entry::submitted(&"🪷".repeat(MAX_ACTIVITY_CHARS)).is_ok());
        assert!(Entry::submitted(&"é".repeat(MAX_ACTIVITY_CHARS + 1)).is_err());
        assert!(Entry::submitted("Reading").unwrap().timestamp_unix_ms > 0);
    }

    #[test]
    fn appends_private_records_without_overwriting_history() {
        let temp = Temp::new();
        let dir = temp.0.join("history");
        append_in(&dir, &entry("Reading \"Rust\" 🪷")).unwrap();
        append_in(&dir, &entry("Taking a break")).unwrap();
        let path = dir.join("activity.jsonl");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let records: Vec<Entry> = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            records,
            [entry("Reading \"Rust\" 🪷"), entry("Taking a break")]
        );
    }

    #[test]
    fn refuses_incomplete_history_and_concurrent_writer() {
        let temp = Temp::new();
        append_in(&temp.0, &entry("Previous")).unwrap();
        let path = temp.0.join("activity.jsonl");
        let file = OpenOptions::new().append(true).open(&path).unwrap();
        // SAFETY: a live descriptor, no pointer arguments.
        assert_eq!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let before = fs::read(&path).unwrap();
        assert!(append_in(&temp.0, &entry("New")).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        drop(file);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{broken")
            .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(append_in(&temp.0, &entry("New")).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn refuses_symlinks_and_hardlinks_without_touching_target() {
        let temp = Temp::new();
        let target = temp.0.join("target");
        fs::write(&target, b"untouched").unwrap();
        let dir = temp.0.join("history");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("activity.jsonl");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(append_in(&dir, &entry("New")).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(append_in(&dir, &entry("New")).is_err());
        let alias = temp.0.join("alias");
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        assert!(append_in(&alias, &entry("New")).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
    }
}
