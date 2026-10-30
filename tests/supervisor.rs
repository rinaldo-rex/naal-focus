//! Native integration: three clients share one supervisor. Never shows overlays.
#![cfg(target_os = "linux")]
use naal_focus::{
    config::{self, Settings},
    engine::{CONTROL_SERVICE, Engine, Request},
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
#[ignore = "requires COSMIC and an isolated session bus/runtime; use scripts/test-supervisor.sh"]
fn monitor_clients_share_controls_and_take_over_without_duplicate_timers() {
    let dir = std::env::temp_dir().join(format!("naal-supervisor-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    fs::write(&path, b"invalid json").unwrap();
    // Reproduce an old applet briefly retaining its lock during an upgrade.
    let startup_lock = Singleton::acquire("applet").unwrap();
    let owner = Engine::start(
        Settings::default(),
        Some(path.clone()),
        Some("Invalid settings".into()),
    );
    wait_until(|| {
        owner
            .status()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("already running"))
    });
    assert!(!owner.status().safe);
    drop(startup_lock);
    wait_until(|| owner.status().safe);
    assert!(!owner.status().settings.enabled);
    assert!(!owner.status().running);
    wait_until(|| Singleton::acquire("focus-tracker").is_err());

    let second = Engine::start(Settings::default(), Some(path.clone()), None);
    let third = Engine::start(Settings::default(), Some(path.clone()), None);
    wait_until(|| second.status().safe && third.status().safe);
    assert!(
        Singleton::acquire("applet").is_err(),
        "one shared supervisor owns the lock"
    );

    second.request(Request::Enabled(false));
    wait_until(|| {
        third
            .status()
            .error
            .as_deref()
            .is_some_and(|e| e.contains("Repair settings"))
    });
    assert_eq!(fs::read(&path).unwrap(), b"invalid json");

    let settings = Settings {
        interval_secs: 60,
        reflections_enabled: false,
        typing_limit_secs: 600,
        // Preferences synchronize without enabling collection of real titles.
        focus: config::FocusSettings {
            enabled: false,
            style: config::FocusStyle::Timeline,
            side: config::FocusSide::Left,
            use_24_hour_time: true,
        },
        breathing: naal_focus::breathing::Settings {
            enabled: true,
            reduced_motion: true,
            pace: naal_focus::breathing::Pace::Even,
            cycles: 2,
            families: vec![
                naal_focus::breathing::Family::Polygons,
                naal_focus::breathing::Family::Weave,
                naal_focus::breathing::Family::EmberBloom,
                naal_focus::breathing::Family::MoonPool,
                naal_focus::breathing::Family::GlassJelly,
            ],
        },
        charts: config::ChartSettings {
            style: config::ChartStyle::Treemap,
            projects: true,
            feelings: false,
            tags: true,
        },
        quotes: vec![
            "A shared reminder.\n\n- A literal bullet.\nKeep these lines together.".into(),
        ],
        ..Settings::default()
    };
    second.request(Request::Settings(settings.clone()));
    wait_until(|| {
        [&owner, &second, &third]
            .iter()
            .all(|engine| engine.status().settings == settings && engine.status().error.is_none())
    });
    assert_eq!(config::load(&path).unwrap(), settings);

    third.request(Request::Snooze(Duration::from_secs(900)));
    wait_until(|| {
        [&owner, &second, &third]
            .iter()
            .all(|engine| engine.status().snoozed_secs.is_some())
    });
    second.request(Request::Resume);
    wait_until(|| {
        [&owner, &second, &third]
            .iter()
            .all(|engine| engine.status().snoozed_secs.is_none())
    });

    // Closing a secondary popup/process must not stop the scheduler.
    drop(second);
    third.request(Request::Snooze(Duration::from_secs(900)));
    wait_until(|| owner.status().snoozed_secs.is_some());

    let bus = zbus::blocking::Connection::session().unwrap();
    let dbus = zbus::blocking::Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .unwrap();
    let original: String = dbus.call("GetNameOwner", &(CONTROL_SERVICE,)).unwrap();
    drop(owner);
    wait_until(|| {
        dbus.call::<_, _, String>("GetNameOwner", &(CONTROL_SERVICE,))
            .is_ok_and(|name| name != original)
    });
    wait_until(|| third.status().safe && third.status().error.is_none());
    assert!(
        third.status().snoozed_secs.is_some(),
        "takeover preserves the snooze"
    );
    assert_eq!(third.status().settings, settings);
    third.request(Request::Resume);
    wait_until(|| third.status().snoozed_secs.is_none());
    assert!(!third.status().running);
    assert!(!third.status().settings.enabled);
    drop(third);
    wait_until(|| Singleton::acquire("applet").is_ok());
    wait_until(|| Singleton::acquire("focus-tracker").is_ok());
    fs::remove_dir_all(dir).unwrap();
}
