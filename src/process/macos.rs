use std::time::Duration;

/// Darwin’s raw monotonic clock includes sleep; uptime clocks do not.
pub fn boot_time() -> Duration {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: time is a live writable timespec and this clock exists on macOS 10.12+.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut time) };
    assert_eq!(result, 0, "A suspend-aware monotonic clock is required");
    Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}
