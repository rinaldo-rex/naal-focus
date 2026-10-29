//! Private data files share directory locks even when individual files are replaced.
use std::{
    ffi::{CStr, CString, OsString},
    fs::{DirBuilder, File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub(crate) fn directory() -> Result<PathBuf, String> {
    directory_from_home(std::env::var_os("HOME"))
}

fn directory_from_home(home: Option<OsString>) -> Result<PathBuf, String> {
    home.map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(".naal-focus"))
        .ok_or_else(|| "An absolute HOME is required to save activity logs.".into())
}

pub(crate) fn existing_dir(directory: &Path) -> Result<Option<File>, String> {
    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)
    {
        Ok(dir) => {
            check_owner(&dir)?;
            Ok(Some(dir))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Opening the directory itself and using openat keeps renamed/replaced paths
/// from redirecting a save elsewhere.
pub(crate) fn private_dir(directory: &Path) -> Result<File, String> {
    match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)
        .map_err(|e| e.to_string())?;
    check_owner(&dir)?;
    dir.set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    Ok(dir)
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

pub(crate) fn check_owner(file: &File) -> Result<(), String> {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    if file.metadata().map_err(|e| e.to_string())?.uid() != unsafe { libc::geteuid() } {
        return Err("Activity history must be owned by the current user.".into());
    }
    Ok(())
}

pub(crate) struct Scratch<'a> {
    pub(crate) dir: &'a File,
    pub(crate) name: CString,
}

impl Scratch<'_> {
    pub(crate) fn replace(&self, file: &File, destination: &CStr) -> Result<(), String> {
        file.sync_all().map_err(|e| e.to_string())?;
        // SAFETY: live directory and two NUL-terminated paths on the same filesystem.
        if unsafe {
            libc::renameat(
                self.dir.as_raw_fd(),
                self.name.as_ptr(),
                self.dir.as_raw_fd(),
                destination.as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        self.dir.sync_all().map_err(|e| e.to_string())
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        // SAFETY: live directory and NUL-terminated, private temporary filename.
        unsafe {
            libc::unlinkat(self.dir.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        os::unix::fs::symlink,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "naal-private-fs-{}-{}",
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

    #[test]
    fn data_directory_requires_an_absolute_home() {
        for home in [
            None,
            Some(OsString::from("relative")),
            Some(OsString::new()),
        ] {
            assert_eq!(
                directory_from_home(home).unwrap_err(),
                "An absolute HOME is required to save activity logs."
            );
        }
        assert_eq!(
            directory_from_home(Some("/tmp/demo-home".into())).unwrap(),
            Path::new("/tmp/demo-home/.naal-focus")
        );
    }

    #[test]
    fn reading_does_not_create_or_chmod_and_writing_is_private() {
        let temp = Temp::new();
        let path = temp.0.join("history");
        assert!(existing_dir(&path).unwrap().is_none());
        assert!(!path.exists());
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(existing_dir(&path).unwrap().is_some());
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o755);
        let dir = private_dir(&path).unwrap();
        assert_eq!(dir.metadata().unwrap().mode() & 0o777, 0o700);
        let file = open_at(&dir, c"entry", libc::O_WRONLY | libc::O_CREAT).unwrap();
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
        check_owner(&file).unwrap();
    }

    #[test]
    fn directory_handles_stay_anchored_and_opens_refuse_symlinks() {
        let temp = Temp::new();
        let path = temp.0.join("history");
        let dir = private_dir(&path).unwrap();
        let moved = temp.0.join("original");
        fs::rename(&path, &moved).unwrap();
        let elsewhere = temp.0.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        symlink(&elsewhere, &path).unwrap();
        assert!(existing_dir(&path).is_err());
        assert!(private_dir(&path).is_err());
        let mut file = open_at(&dir, c"entry", libc::O_WRONLY | libc::O_CREAT).unwrap();
        file.write_all(b"anchored").unwrap();
        assert_eq!(fs::read(moved.join("entry")).unwrap(), b"anchored");
        assert!(!elsewhere.join("entry").exists());
        let outside = temp.0.join("outside");
        fs::write(&outside, b"untouched").unwrap();
        symlink(&outside, moved.join("link")).unwrap();
        assert!(open_at(&dir, c"link", libc::O_WRONLY | libc::O_CREAT).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"untouched");
        // SAFETY: live descriptor and F_GETFD/F_GETFL have no variadic argument.
        let (directory_flags, file_flags, status_flags) = unsafe {
            (
                libc::fcntl(dir.as_raw_fd(), libc::F_GETFD),
                libc::fcntl(file.as_raw_fd(), libc::F_GETFD),
                libc::fcntl(file.as_raw_fd(), libc::F_GETFL),
            )
        };
        assert!(directory_flags >= 0 && file_flags >= 0 && status_flags >= 0);
        assert_ne!(directory_flags & libc::FD_CLOEXEC, 0);
        assert_ne!(file_flags & libc::FD_CLOEXEC, 0);
        assert_ne!(status_flags & libc::O_NONBLOCK, 0);
    }

    #[test]
    fn directory_locks_share_reads_and_refuse_conflicting_access() {
        let temp = Temp::new();
        let shared_path = temp.0.join("shared");
        let writer = private_dir(&shared_path).unwrap();
        let first = existing_dir(&shared_path).unwrap().unwrap();
        let second = existing_dir(&shared_path).unwrap().unwrap();
        lock_shared(&first).unwrap();
        lock_shared(&second).unwrap();
        assert_eq!(
            lock(&writer).unwrap_err(),
            "Activity history is busy; try again."
        );
        let exclusive_path = temp.0.join("exclusive");
        let owner = private_dir(&exclusive_path).unwrap();
        lock(&owner).unwrap();
        let contender = existing_dir(&exclusive_path).unwrap().unwrap();
        assert_eq!(
            lock(&contender).unwrap_err(),
            "Activity history is busy; try again."
        );
        assert_eq!(
            lock_shared(&contender).unwrap_err(),
            "Activity history is busy; try again."
        );
    }

    #[test]
    fn replacement_preserves_old_readers_and_the_directory_lock() {
        let temp = Temp::new();
        let dir = private_dir(&temp.0).unwrap();
        lock(&dir).unwrap();
        let path = temp.0.join("target");
        fs::write(&path, b"old").unwrap();
        let mut reader = File::open(&path).unwrap();
        let scratch = Scratch {
            dir: &dir,
            name: c".scratch.tmp".to_owned(),
        };
        let mut file = open_at(
            &dir,
            &scratch.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        file.write_all(b"new").unwrap();
        scratch.replace(&file, c"target").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let mut previous = Vec::new();
        reader.read_to_end(&mut previous).unwrap();
        assert_eq!(previous, b"old");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let contender = existing_dir(&temp.0).unwrap().unwrap();
        assert_eq!(
            lock_shared(&contender).unwrap_err(),
            "Activity history is busy; try again."
        );
        drop(scratch);
        assert!(!temp.0.join(".scratch.tmp").exists());
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
    }

    #[test]
    fn abandoned_or_failed_replacements_remove_only_the_scratch() {
        let temp = Temp::new();
        let dir = private_dir(&temp.0).unwrap();
        let target = temp.0.join("target");
        fs::write(&target, b"old").unwrap();
        let scratch = Scratch {
            dir: &dir,
            name: c".scratch.tmp".to_owned(),
        };
        let mut file = open_at(
            &dir,
            &scratch.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        file.write_all(b"unfinished").unwrap();
        drop(scratch);
        assert!(!temp.0.join(".scratch.tmp").exists());
        assert_eq!(fs::read(&target).unwrap(), b"old");
        let blocked = temp.0.join("blocked");
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("keep"), b"untouched").unwrap();
        let scratch = Scratch {
            dir: &dir,
            name: c".scratch.tmp".to_owned(),
        };
        let file = open_at(
            &dir,
            &scratch.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        assert!(scratch.replace(&file, c"blocked").is_err());
        drop(scratch);
        assert!(!temp.0.join(".scratch.tmp").exists());
        assert_eq!(fs::read(blocked.join("keep")).unwrap(), b"untouched");
        assert_eq!(fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn sync_failure_keeps_the_target_and_cleans_the_scratch() {
        let temp = Temp::new();
        let dir = private_dir(&temp.0).unwrap();
        let target = temp.0.join("target");
        fs::write(&target, b"old").unwrap();
        let scratch = Scratch {
            dir: &dir,
            name: c".scratch.tmp".to_owned(),
        };
        let _file = open_at(
            &dir,
            &scratch.name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        // Linux character devices cannot be fsynced: fail before the rename.
        let unsyncable = File::open("/dev/null").unwrap();
        assert!(scratch.replace(&unsyncable, c"target").is_err());
        drop(scratch);
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!temp.0.join(".scratch.tmp").exists());
    }
}
