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
    sync::mpsc::{self, Receiver, TryRecvError},
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

/// One in-flight durable save. Polling never blocks the input/render loop.
pub struct PendingSave {
    result: Receiver<Result<(), String>>,
    done: bool,
}
impl PendingSave {
    pub fn start(entry: Entry) -> Result<Self, String> {
        Self::start_with(entry, |entry| save(&entry))
    }
    fn start_with(
        entry: Entry,
        write: impl FnOnce(Entry) -> Result<(), String> + Send + 'static,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("activity-save".into())
            .spawn(move || {
                let _ = tx.send(write(entry));
            })
            .map_err(|_| "Cannot start activity save; try again.")?;
        Ok(Self {
            result: rx,
            done: false,
        })
    }
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        if self.done {
            return None;
        }
        let result = match self.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Err("Activity save worker stopped; inspect the log before retrying.".into())
            }
        };
        self.done = true;
        Some(result)
    }
}

pub fn save(entry: &Entry) -> Result<(), String> {
    append_in(&directory()?, entry).map_err(|e| format!("Activity save failed: {e}"))
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
    let created = match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(e.to_string()),
    };
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
    if created {
        // Also persist the new .naal-focus entry in HOME before acknowledging.
        File::open(
            directory
                .parent()
                .ok_or("Missing activity directory parent.")?,
        )
        .and_then(|parent| parent.sync_all())
        .map_err(|e| e.to_string())?;
    }
    append_transaction(&mut file, original_len, |file| file.write_all(&bytes))
}

fn append_transaction(
    file: &mut File,
    original_len: u64,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> Result<(), String> {
    if let Err(error) = write(file).and_then(|()| file.sync_all()) {
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
    fn worker_acknowledges_once_and_only_after_persistence() {
        let temp = Temp::new();
        let directory = temp.0.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (go_tx, go_rx) = mpsc::sync_channel(1);
        let mut pending = PendingSave::start_with(entry("Reading"), move |entry| {
            ready_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            append_in(&directory, &entry)
        })
        .unwrap();
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(pending.poll().is_none());
        assert!(!temp.0.join("activity.jsonl").exists());
        go_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(result) = pending.poll() {
                result.unwrap();
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(pending.poll().is_none());
        assert_eq!(
            fs::read_to_string(temp.0.join("activity.jsonl"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn partial_write_failure_rolls_back_only_the_new_record() {
        let temp = Temp::new();
        append_in(&temp.0, &entry("Previous")).unwrap();
        let path = temp.0.join("activity.jsonl");
        let before = fs::read(&path).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        let result = append_transaction(&mut file, before.len() as u64, |file| {
            file.write_all(b"{partial")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        append_in(&temp.0, &entry("Retry")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
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
