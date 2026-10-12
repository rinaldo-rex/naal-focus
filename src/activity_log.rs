//! Private activity history with idempotent appends and atomic corrections.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    fs::{self, DirBuilder, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MAX_ACTIVITY_CHARS: usize = 2000;
const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u32,
    /// Opaque identity; default preserves compatibility with pre-ID records.
    #[serde(default)]
    pub id: String,
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
            id: random_id()?,
            timestamp_unix_ms,
            text: text.to_owned(),
        })
    }
    pub fn corrected(&self, text: &str) -> Result<Self, String> {
        validate_text(text)?;
        Ok(Self {
            text: text.to_owned(),
            ..self.clone()
        })
    }
}
fn random_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| "Cannot generate an activity identity.")?;
    Ok(bytes.into_iter().map(|b| format!("{b:02x}")).collect())
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
    pub fn start_correction(original: Entry, entry: Entry) -> Result<Self, String> {
        Self::start_with(entry, move |entry| {
            persist_in(&directory()?, Some(&original), &entry)
                .map_err(|e| format!("Activity correction failed: {e}"))
        })
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
    persist_in(directory, None, entry)
}
fn persist_in(directory: &Path, original: Option<&Entry>, entry: &Entry) -> Result<(), String> {
    validate_text(&entry.text)?;
    if entry.id.len() != 32 || !entry.id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid activity identity.".into());
    }
    if original
        .is_some_and(|old| old.id != entry.id || old.timestamp_unix_ms != entry.timestamp_unix_ms)
    {
        return Err("Corrections must preserve the original identity and timestamp.".into());
    }
    if entry.version != 1 {
        return Err("Unsupported activity log version.".into());
    }
    let mut bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let dir = private_dir(directory)?;
    // The directory lock remains stable when corrections replace activity.jsonl.
    lock(&dir)?;
    let mut file = open_at(
        &dir,
        c"activity.jsonl",
        libc::O_RDWR | libc::O_CREAT | libc::O_APPEND,
    )
    .map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    check_owner(&file)?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("Activity history must be a regular, unlinked-to-other-files file.".into());
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    lock(&file)?;
    let original_len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut found = None;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(&mut file);
    while let Some((_, record)) = read_record(&mut reader)? {
        if record.id == entry.id {
            if found.is_some() {
                return Err("Duplicate activity identity; repair history before saving.".into());
            }
            found = Some(record);
        }
    }
    // Persist directory creation even on retries after a previous sync failure.
    File::open(
        directory
            .parent()
            .ok_or("Missing activity directory parent.")?,
    )
    .and_then(|parent| parent.sync_all())
    .map_err(|e| e.to_string())?;
    dir.sync_all().map_err(|e| e.to_string())?;
    if found.as_ref() == Some(entry) {
        // Retry after an uncertain acknowledgment never creates another record.
        return file.sync_all().map_err(|e| e.to_string());
    }
    match original {
        None if found.is_none() => {
            append_transaction(&mut file, original_len, |file| file.write_all(&bytes))
        }
        Some(original) if found.as_ref() == Some(original) => {
            replace_record(&dir, &mut file, entry, &bytes)
        }
        _ => Err("Activity entry changed or disappeared; existing history was preserved.".into()),
    }
}

/// The private `~/.naal-focus/` directory with link and owner guards.
/// Opening it follows neither symlinks nor renamed/replaced paths.
pub(crate) fn private_dir(directory: &Path) -> Result<File, String> {
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
    Ok(dir)
}

/// Lenient read-only history scan for read-only consumers such as the
/// dashboard. Malformed, incomplete, oversized, duplicate, or blank lines
/// are skipped individually and counted; valid entries still load. A missing
/// directory or history is an empty history, not an error. Sharing the
/// writer's lock keeps a partial append or correction out of a scan.
pub fn load_lenient(directory: &Path) -> Result<(Vec<Entry>, usize), String> {
    load_lenient_matching(directory, |_| true)
}

/// Filter during scanning so a reminder retains only today's records, not an
/// entire archive. Like `load_lenient`, this must run off the input thread.
pub(crate) fn load_lenient_matching(
    directory: &Path,
    keep: impl Fn(&Entry) -> bool,
) -> Result<(Vec<Entry>, usize), String> {
    let dir = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)
    {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e.to_string()),
    };
    check_owner(&dir)?;
    lock_shared(&dir)?;
    let mut file = match open_at(&dir, c"activity.jsonl", libc::O_RDONLY) {
        Ok(file) => file,
        // A missing history is an empty history, not a read error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(format!("Activity history is unreadable: {e}")),
    };
    check_owner(&file)?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Activity history must be a regular file.".into());
    }
    lock_shared(&file)?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut skipped = 0;
    let mut seen = HashSet::new();
    let mut inner = BufReader::new(&file);
    let reader = &mut inner;
    loop {
        let mut line = Vec::new();
        let bytes = reader
            .take(MAX_RECORD_BYTES + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if bytes == 0 {
            break;
        }
        if line.len() as u64 > MAX_RECORD_BYTES {
            // Drain in bounded chunks. A newline already in the bounded prefix
            // belongs to this line; never accidentally drain the next good row.
            if line.last() != Some(&b'\n') {
                loop {
                    let chunk = reader.fill_buf().map_err(|e| e.to_string())?;
                    if chunk.is_empty() {
                        break;
                    }
                    let end = chunk.iter().position(|byte| *byte == b'\n');
                    let consumed = end.map_or(chunk.len(), |index| index + 1);
                    reader.consume(consumed);
                    if end.is_some() {
                        break;
                    }
                }
            }
            skipped += 1;
            continue;
        }
        if line.last() != Some(&b'\n') {
            // Incomplete final record: it was never acknowledged as saved.
            skipped += 1;
            break;
        }
        let record: Option<Entry> = serde_json::from_slice(&line)
            .ok()
            .filter(|record: &Entry| record.version == 1 && validate_text(&record.text).is_ok());
        match record {
            Some(record) if !keep(&record) => {}
            Some(record) if record.id.is_empty() => entries.push(record),
            Some(record) if seen.insert(record.id.clone()) => entries.push(record),
            _ => skipped += 1,
        }
    }
    Ok((entries, skipped))
}

pub(crate) fn lock(file: &File) -> Result<(), String> {
    // SAFETY: a live descriptor; flock has no pointer arguments. Linux supports
    // advisory locks on directory descriptors as well as regular files.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Activity history is busy; try again.".into());
    }
    Ok(())
}
pub(crate) fn lock_shared(file: &File) -> Result<(), String> {
    // SAFETY: a live descriptor; flock has no pointer arguments. Linux supports
    // advisory locks on directory descriptors as well as regular files.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return Err("Activity history is busy; try again.".into());
    }
    Ok(())
}
pub(crate) fn open_at(dir: &File, name: &CStr, flags: libc::c_int) -> Result<File, std::io::Error> {
    // SAFETY: live directory and NUL-terminated name; mode accompanies O_CREAT.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the newly owned descriptor is transferred exactly once.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn read_record(reader: &mut impl BufRead) -> Result<Option<(Vec<u8>, Entry)>, String> {
    let mut line = Vec::new();
    let bytes = reader
        .take(MAX_RECORD_BYTES + 1)
        .read_until(b'\n', &mut line)
        .map_err(|e| e.to_string())?;
    if bytes == 0 {
        return Ok(None);
    }
    if bytes as u64 > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
        return Err("Activity history has an incomplete or oversized record; repair it before retrying. Existing data was preserved.".into());
    }
    let record: Entry = serde_json::from_slice(&line)
        .map_err(|_| "Activity history has a malformed record; repair it before retrying.")?;
    validate_text(&record.text)?;
    if record.version != 1 {
        return Err("Unsupported activity history version.".into());
    }
    Ok(Some((line, record)))
}
struct Scratch<'a> {
    dir: &'a File,
    name: CString,
}
impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        // SAFETY: live directory and NUL-terminated, private temporary filename.
        unsafe {
            libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}
fn replace_record(dir: &File, file: &mut File, entry: &Entry, bytes: &[u8]) -> Result<(), String> {
    let scratch = Scratch {
        dir,
        name: CString::new(format!(".activity-{}.tmp", random_id()?)).map_err(|e| e.to_string())?,
    };
    let mut output = open_at(
        dir,
        &scratch.name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )
    .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    while let Some((line, record)) = read_record(&mut reader)? {
        output
            .write_all(if record.id == entry.id { bytes } else { &line })
            .map_err(|e| e.to_string())?;
    }
    output.sync_all().map_err(|e| e.to_string())?;
    // SAFETY: live directory and two NUL-terminated paths on the same filesystem.
    if unsafe {
        libc::renameat(
            dir.as_raw_fd(),
            scratch.name.as_ptr(),
            dir.as_raw_fd(),
            c"activity.jsonl".as_ptr(),
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    dir.sync_all().map_err(|e| e.to_string())
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

pub(crate) fn check_owner(file: &File) -> Result<(), String> {
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
            id: {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                text.hash(&mut hash);
                format!("{:032x}", hash.finish())
            },
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
    fn corrections_are_atomic_idempotent_and_keep_original_timestamp() {
        let temp = Temp::new();
        let first = entry("Reading docs");
        let other = entry("Taking a break");
        append_in(&temp.0, &first).unwrap();
        append_in(&temp.0, &other).unwrap();
        append_in(&temp.0, &first).unwrap(); // uncertain-ack retry: not another entry
        let corrected = first.corrected("Reviewing Rust documentation").unwrap();
        persist_in(&temp.0, Some(&first), &corrected).unwrap();
        persist_in(&temp.0, Some(&first), &corrected).unwrap(); // correction retry
        let records: Vec<Entry> = fs::read_to_string(temp.0.join("activity.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records, [corrected.clone(), other]);
        assert_eq!(corrected.id, first.id);
        assert_eq!(corrected.timestamp_unix_ms, first.timestamp_unix_ms);
        assert_eq!(
            fs::metadata(temp.0.join("activity.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1); // no abandoned temporary file
    }

    #[test]
    fn correction_conflicts_and_directory_lock_preserve_history() {
        let temp = Temp::new();
        let original = entry("Reading");
        append_in(&temp.0, &original).unwrap();
        let updated = original.corrected("Coding").unwrap();
        persist_in(&temp.0, Some(&original), &updated).unwrap();
        let path = temp.0.join("activity.jsonl");
        let before = fs::read(&path).unwrap();
        assert!(
            persist_in(
                &temp.0,
                Some(&original),
                &original.corrected("Stale edit").unwrap()
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        let dir_lock = File::open(&temp.0).unwrap();
        lock(&dir_lock).unwrap();
        assert!(append_in(&temp.0, &entry("New")).is_err());
        assert!(
            persist_in(
                &temp.0,
                Some(&updated),
                &updated.corrected("Again").unwrap()
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn legacy_records_survive_corrections_and_malformed_history_is_preserved() {
        let temp = Temp::new();
        let original = entry("Reading");
        append_in(&temp.0, &original).unwrap();
        let path = temp.0.join("activity.jsonl");
        let legacy = b"{\"version\":1,\"timestamp_unix_ms\":10,\"text\":\"Legacy\"}\n";
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(legacy)
            .unwrap();
        let updated = original.corrected("Coding").unwrap();
        persist_in(&temp.0, Some(&original), &updated).unwrap();
        assert!(fs::read(&path).unwrap().ends_with(legacy));
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"malformed\n")
            .unwrap();
        let before = fs::read(&path).unwrap();
        assert!(append_in(&temp.0, &entry("New")).is_err());
        assert!(
            persist_in(
                &temp.0,
                Some(&updated),
                &updated.corrected("Again").unwrap()
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), before);
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
    fn lenient_scan_skips_bad_lines_and_keeps_valid_entries() {
        let temp = Temp::new();
        let dir = temp.0.join("history");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("activity.jsonl");
        let legacy = br#"{"version":1,"timestamp_unix_ms":10,"text":"Legacy"}"#;
        let good =
            br#"{"version":1,"id":"0123456789abcdef0123456789abcdef","timestamp_unix_ms":20,"text":"Reading"}"#;
        let mut file = fs::File::create(&path).unwrap();
        for line in [legacy as &[u8], good, good] {
            file.write_all(line).unwrap();
            file.write_all(b"\n").unwrap();
        }
        file.write_all(b"malformed\n\n").unwrap();
        file.write_all(b"{\"version\":2,\"timestamp_unix_ms\":1,\"text\":\"x\"}\n")
            .unwrap();
        file.write_all(b"{\"version\":1,\"timestamp_unix_ms\":3,\"text\":\"a\\nb\"}\n")
            .unwrap();
        let (entries, skipped) = load_lenient(&dir).unwrap();
        let legacy_entry = Entry {
            version: 1,
            id: String::new(),
            timestamp_unix_ms: 10,
            text: "Legacy".into(),
        };
        let good_entry = Entry {
            version: 1,
            id: "0123456789abcdef0123456789abcdef".into(),
            timestamp_unix_ms: 20,
            text: "Reading".into(),
        };
        assert_eq!(
            entries,
            [legacy_entry, good_entry],
            "duplicate, malformed, blank, version, and control-character lines are skipped"
        );
        assert_eq!(skipped, 5);
        // A missing directory or history is simply empty, never an error.
        assert_eq!(
            load_lenient(&temp.0.join("absent")).unwrap(),
            (Vec::new(), 0)
        );
        let empty = temp.0.join("empty");
        fs::create_dir(&empty).unwrap();
        assert_eq!(load_lenient(&empty).unwrap(), (Vec::new(), 0));
    }

    #[test]
    fn oversized_scan_lines_are_bounded_and_do_not_swallow_following_records() {
        let temp = Temp::new();
        let good = entry("Good");
        let mut bytes = vec![b'x'; MAX_RECORD_BYTES as usize];
        bytes.push(b'\n'); // newline is exactly the last byte of the bounded prefix
        bytes.extend(serde_json::to_vec(&good).unwrap());
        bytes.push(b'\n');
        bytes.extend(vec![b'x'; MAX_RECORD_BYTES as usize * 4]);
        bytes.push(b'\n');
        let other = entry("Other");
        bytes.extend(serde_json::to_vec(&other).unwrap());
        bytes.push(b'\n');
        fs::write(temp.0.join("activity.jsonl"), bytes).unwrap();
        assert_eq!(load_lenient(&temp.0).unwrap(), (vec![good, other], 2));
    }
    #[test]
    fn filtered_recall_scan_keeps_only_matching_records_without_modifying_history() {
        let temp = Temp::new();
        let old = entry("Yesterday");
        let mut today = entry("Today 🪷");
        today.timestamp_unix_ms = 86_400_000;
        append_in(&temp.0, &old).unwrap();
        append_in(&temp.0, &today).unwrap();
        let path = temp.0.join("activity.jsonl");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"malformed\n")
            .unwrap();
        let before = fs::read(&path).unwrap();
        let (entries, skipped) =
            load_lenient_matching(&temp.0, |entry| entry.timestamp_unix_ms >= 86_400_000).unwrap();
        assert_eq!(entries, [today]);
        assert_eq!(skipped, 1);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    #[test]
    fn reusing_text_appends_a_new_moment_and_correction_preserves_the_past() {
        let temp = Temp::new();
        let previous = entry("Reviewing proposal");
        append_in(&temp.0, &previous).unwrap();
        let reused = Entry::submitted(&previous.text).unwrap();
        assert_ne!(reused.id, previous.id);
        assert!(reused.timestamp_unix_ms > previous.timestamp_unix_ms);
        append_in(&temp.0, &reused).unwrap();
        let corrected = reused.corrected("Reviewing updated proposal").unwrap();
        persist_in(&temp.0, Some(&reused), &corrected).unwrap();
        assert_eq!(load_lenient(&temp.0).unwrap().0, [previous, corrected]);
    }
    #[test]
    fn lenient_scan_refuses_links_foreign_owners_and_writers() {
        let temp = Temp::new();
        let dir = temp.0.join("history");
        append_in(&dir, &entry("Reading")).unwrap();
        let writer = File::open(&dir).unwrap();
        lock(&writer).unwrap();
        assert!(load_lenient(&dir).is_err());
        drop(writer);
        let target = temp.0.join("target");
        fs::write(&target, b"untouched").unwrap();
        let alias = temp.0.join("alias");
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        assert!(load_lenient(&alias).is_err());
        let path = dir.join("activity.jsonl");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(load_lenient(&dir).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
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
