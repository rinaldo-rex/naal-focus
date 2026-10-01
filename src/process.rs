//! Linux process and compositor routing boundaries shared by the applet and helper.
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt},
    },
    path::PathBuf,
    process::{Child, Command},
    time::Duration,
};

pub fn runtime_dir() -> Result<PathBuf, String> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "An absolute XDG_RUNTIME_DIR is required.".into())
}

pub fn desktop_socket_path() -> Result<PathBuf, String> {
    let display = std::env::var_os("NAAL_FOCUS_WAYLAND_DISPLAY")
        .or_else(|| std::env::var_os("WAYLAND_DISPLAY"))
        .ok_or("WAYLAND_DISPLAY is unavailable; cannot identify the desktop compositor.")?;
    let path = PathBuf::from(display);
    if path.is_absolute() {
        return Ok(path);
    }
    if path.components().count() != 1 {
        return Err("WAYLAND_DISPLAY must be a socket name or absolute path.".into());
    }
    Ok(runtime_dir()?.join(path))
}

/// Never consume WAYLAND_SOCKET: COSMIC applets inherit the nested panel socket.
pub fn desktop_connection() -> Result<wayland_client::Connection, String> {
    let path = desktop_socket_path()?;
    let socket = UnixStream::connect(&path)
        .map_err(|e| format!("Desktop socket {}: {e}", path.display()))?;
    wayland_client::Connection::from_socket(socket).map_err(|e| e.to_string())
}

pub struct Singleton {
    _file: File,
}
impl Singleton {
    pub fn acquire(name: &str) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(runtime_dir()?.join(format!("naal-focus-{name}.lock")))
            .map_err(|e| e.to_string())?;
        // SAFETY: the descriptor is valid for the lifetime of file, and flock has no pointer arguments.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(format!("Another Naal Focus {name} is already running."));
        }
        Ok(Self { _file: file })
    }
}

pub fn boot_time() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: ts points to a live writable timespec; CLOCK_BOOTTIME is supported on Linux.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    assert_eq!(result, 0, "CLOCK_BOOTTIME is required");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

pub fn helper_command() -> Result<Command, String> {
    let path = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("naal-focus-overlay");
    if !path.is_file() {
        return Err(format!("Missing overlay helper: {}", path.display()));
    }
    let mut cmd = Command::new(path);
    cmd.env_remove("WAYLAND_SOCKET")
        .env_remove("X_PRIVILEGED_WAYLAND_SOCKET");
    // Validate the route before any focus-capturing process is created.
    cmd.env("NAAL_FOCUS_WAYLAND_DISPLAY", desktop_socket_path()?);
    // SAFETY: the pre_exec closure only uses async-signal-safe Linux syscalls; no allocations or locks.
    unsafe {
        cmd.pre_exec(|| {
            let parent = libc::getppid();
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if parent == 1 || libc::getppid() != parent {
                libc::_exit(1);
            }
            Ok(())
        });
    }
    Ok(cmd)
}

/// Always kill and reap a still-running helper when its supervisor disappears.
pub struct ManagedChild(pub Child);
impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boot_clock_is_monotonic() {
        assert!(boot_time() <= boot_time());
    }

    #[test]
    fn child_is_killed_and_reaped_on_drop() {
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        drop(ManagedChild(child));
        // SAFETY: signal 0 only checks process existence.
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }

    #[test]
    fn singleton_releases_without_deleting_its_inode() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            return;
        }
        let name = format!("test-{}", std::process::id());
        let lock = Singleton::acquire(&name).unwrap();
        assert!(Singleton::acquire(&name).is_err());
        drop(lock);
        assert!(Singleton::acquire(&name).is_ok());
        std::fs::remove_file(
            runtime_dir()
                .unwrap()
                .join(format!("naal-focus-{name}.lock")),
        )
        .unwrap();
    }
}
