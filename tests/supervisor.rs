//! Read-only native integration test. Never requests an overlay or enables a timer.
use naal_focus::{
    config::{self, Settings},
    engine::{Engine, Request},
    process::{Singleton, boot_time},
};
use std::{fs, time::Duration};

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = boot_time() + Duration::from_secs(8);
    while !condition() {
        assert!(
            boot_time() < deadline,
            "supervisor did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires an active COSMIC session and no running Naal Focus applet; creates no overlay"]
fn native_supervisor_persists_settings_and_guards_duplicates() {
    let dir = std::env::temp_dir().join(format!("naal-supervisor-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    fs::write(&path, b"invalid json").unwrap();
    let engine = Engine::start(
        Settings::default(),
        Some(path.clone()),
        Some("Invalid settings".into()),
    );
    wait_until(|| engine.status().safe);
    assert!(!engine.status().settings.enabled);
    assert!(!engine.status().running);

    let duplicate = Engine::start(Settings::default(), Some(path.clone()), None);
    wait_until(|| duplicate.status().error.is_some());
    assert!(
        duplicate
            .status()
            .error
            .unwrap()
            .contains("already running")
    );
    drop(duplicate);

    engine.request(Request::Enabled(false));
    wait_until(|| {
        engine
            .status()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("Repair settings"))
    });
    assert_eq!(fs::read(&path).unwrap(), b"invalid json");

    let settings = Settings {
        interval_secs: 60,
        quotes: vec!["A saved reminder.".into()],
        ..Settings::default()
    };
    engine.request(Request::Settings(settings.clone()));
    wait_until(|| engine.status().settings == settings && engine.status().error.is_none());
    assert_eq!(config::load(&path).unwrap(), settings);

    engine.request(Request::Snooze(Duration::from_secs(900)));
    wait_until(|| engine.status().snoozed_secs.is_some());
    engine.request(Request::Resume);
    wait_until(|| engine.status().snoozed_secs.is_none());
    assert!(!engine.status().running);
    assert!(!engine.status().settings.enabled);
    drop(engine);
    wait_until(|| Singleton::acquire("applet").is_ok());
    fs::remove_dir_all(dir).unwrap();
}
