//! One private, atomically replaced daily file; no archive of window titles.
use super::{Day, MAX_BYTES};
use crate::{
    private_fs::{self, Scratch},
    time::to_local,
};
use std::{
    ffi::CString,
    fs::{File, Permissions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::Path,
};

const NAME: &std::ffi::CStr = c"focus.json";

fn check_file(file: &File) -> Result<(), String> {
    private_fs::check_owner(file)?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err("Focus history must be a regular file with a single hard link.".into());
    }
    Ok(())
}

/// Also purges a previous day's file, even when collection is disabled. Missing
/// history never creates a directory. Run only on a background worker.
pub fn load_today_in(directory: &Path, now_ms: u64) -> Result<Day, String> {
    let Some(dir) = private_fs::existing_dir(directory)? else {
        return Ok(Day::empty(now_ms));
    };
    let dir = private_fs::lock(dir)?;
    // A crash between writing and renaming must not leave a hidden archive of
    // titles. Only our reserved, owner-checked regular scratch files are removed.
    let mut removed_scratch = false;
    for name in private_fs::entries(&dir).map_err(|e| e.to_string())? {
        let name = name.map_err(|e| e.to_string())?;
        let bytes = name.as_bytes();
        if !bytes.starts_with(b".focus-") || !bytes.ends_with(b".tmp") {
            continue;
        }
        let file = private_fs::open_at(&dir, &name, libc::O_RDONLY).map_err(|e| e.to_string())?;
        check_file(&file)?;
        // SAFETY: live directory descriptor and checked NUL-terminated name.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        removed_scratch = true;
    }
    if removed_scratch {
        dir.sync_all().map_err(|e| e.to_string())?;
    }
    let mut file = match private_fs::open_at(&dir, NAME, libc::O_RDONLY) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Day::empty(now_ms)),
        Err(e) => return Err(e.to_string()),
    };
    check_file(&file)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("Focus history is too large.".into());
    }
    file.set_permissions(Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Focus history is too large.".into());
    }
    let day: Day = serde_json::from_slice(&bytes).map_err(|_| "Focus history is unreadable.")?;
    if day.date != to_local(now_ms).date_key() {
        // SAFETY: live directory descriptor and a static NUL-terminated filename.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), NAME.as_ptr(), 0) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        dir.sync_all().map_err(|e| e.to_string())?;
        return Ok(Day::empty(now_ms));
    }
    day.validate(now_ms)?;
    Ok(day)
}

pub fn save_in(directory: &Path, day: &Day, now_ms: u64) -> Result<(), String> {
    day.validate(now_ms)?;
    if day.date != to_local(now_ms).date_key() {
        return Err("Refusing to retain another day's window history.".into());
    }
    let bytes = serde_json::to_vec(day).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Today's focus history is full.".into());
    }
    let dir = private_fs::lock(private_fs::private_dir(directory)?)?;
    match private_fs::open_at(&dir, NAME, libc::O_RDONLY) {
        Ok(file) => check_file(&file)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let scratch = Scratch {
        dir: &dir,
        name: CString::new(format!(
            ".focus-{}-{}.tmp",
            std::process::id(),
            crate::process::boot_time().as_nanos()
        ))
        .map_err(|e| e.to_string())?,
    };
    let mut file = private_fs::open_at(
        &dir,
        &scratch.name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )
    .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    scratch.replace(&file, NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::{Clock, Window};
    use std::{
        fs,
        os::unix::fs::symlink,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const NOW: u64 = 1_792_000_000_000;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "naal-focus-store-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn day() -> Day {
        let mut day = Day::empty(NOW);
        let mut clock = Clock::default();
        for second in 0..3 {
            clock
                .observe(
                    &mut day,
                    Duration::from_secs(second),
                    NOW + second * 1000,
                    Some(Window::new("herdr", "Private project")),
                    true,
                )
                .unwrap();
        }
        day
    }
    #[test]
    fn private_round_trip_restart_and_atomic_replacement() {
        let temp = Temp::new();
        let original = day();
        assert_eq!(load_today_in(&temp.0, NOW).unwrap().total_ms(), 0);
        assert!(!temp.0.exists());
        save_in(&temp.0, &original, NOW + 2000).unwrap();
        assert_eq!(load_today_in(&temp.0, NOW + 2000).unwrap(), original);
        assert_eq!(fs::metadata(&temp.0).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(temp.0.join("focus.json")).unwrap().mode() & 0o777,
            0o600
        );
        let empty = Day::empty(NOW + 3000);
        save_in(&temp.0, &empty, NOW + 3000).unwrap();
        assert_eq!(load_today_in(&temp.0, NOW + 3000).unwrap(), empty);
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }
    #[test]
    fn crash_scratch_files_do_not_become_a_hidden_archive() {
        let temp = Temp::new();
        fs::create_dir(&temp.0).unwrap();
        fs::write(temp.0.join(".focus-old.tmp"), b"old private title").unwrap();
        fs::write(temp.0.join("unrelated.txt"), b"keep").unwrap();
        load_today_in(&temp.0, NOW).unwrap();
        assert!(!temp.0.join(".focus-old.tmp").exists());
        assert!(temp.0.join("unrelated.txt").exists());
    }

    #[test]
    fn old_titles_are_deleted_on_next_day_even_without_collection() {
        let temp = Temp::new();
        save_in(&temp.0, &day(), NOW + 2000).unwrap();
        let fresh = load_today_in(&temp.0, NOW + 86_400_000).unwrap();
        assert!(fresh.sessions.is_empty());
        assert!(!temp.0.join("focus.json").exists());
        assert!(save_in(&temp.0, &day(), NOW + 86_400_000).is_err());
    }
    #[test]
    fn refuses_symlinks_hardlinks_nonregular_files_and_busy_storage() {
        let temp = Temp::new();
        fs::create_dir(&temp.0).unwrap();
        let target = temp.0.join("target");
        fs::write(&target, b"untouched").unwrap();
        let path = temp.0.join("focus.json");
        symlink(&target, &path).unwrap();
        assert!(save_in(&temp.0, &day(), NOW + 2000).is_err());
        assert!(load_today_in(&temp.0, NOW + 2000).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&target, &path).unwrap();
        assert!(save_in(&temp.0, &day(), NOW + 2000).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(load_today_in(&temp.0, NOW + 2000).is_err());
        fs::remove_dir(&path).unwrap();
        let dir = private_fs::private_dir(&temp.0).unwrap();
        let _dir = private_fs::lock(dir).unwrap();
        assert!(save_in(&temp.0, &day(), NOW + 2000).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
    }
    #[test]
    fn malformed_or_future_history_is_not_reported_as_empty() {
        let temp = Temp::new();
        fs::create_dir(&temp.0).unwrap();
        fs::write(temp.0.join("focus.json"), b"not json").unwrap();
        assert!(load_today_in(&temp.0, NOW).is_err());
        save_in(&temp.0, &day(), NOW + 2000).unwrap();
        assert!(load_today_in(&temp.0, NOW).is_err());
    }
}
