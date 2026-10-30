#![cfg(all(target_os = "macos", feature = "applet"))]

use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn app() -> Command {
    Command::new(env!("CARGO_BIN_EXE_naal-focus"))
}

#[test]
fn help_does_not_start_a_desktop_session() {
    let output = app().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("experimental"));
}

#[test]
fn settings_roundtrip_uses_native_controls_without_showing_windows() {
    let output = app().arg("--smoke-test").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unsupervised_helper_rejects_missing_payload_without_showing_windows() {
    let result = app()
        .arg("--overlay")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Missing or oversized"));
}

#[test]
fn helper_watchdog_recovers_a_stalled_startup_without_appkit() {
    let mut child = app()
        .arg("--overlay")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Keep stdin open but send nothing. No payload means no AppKit or reminder.
    let input = child.stdin.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("Independent startup watchdog did not terminate the helper");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(input);
}
