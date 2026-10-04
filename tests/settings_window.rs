//! Real settings-process regression on an isolated bus/runtime. No overlays.
#![cfg(feature = "applet")]
use naal_focus::{
    config::{self, Settings},
    engine::{CONTROL_SERVICE, Engine, Request},
    process::{ManagedChild, Singleton},
};
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "settings process did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "opens a settings window; requires isolated bus/runtime; use just check-settings"]
fn concurrent_launches_reuse_one_window_and_closing_it_leaves_supervisor_running() {
    let dir = naal_focus::process::runtime_dir()
        .unwrap()
        .join("settings-test");
    let home = dir.join("home");
    let config_dir = dir.join("config");
    fs::create_dir_all(&home).unwrap();
    let path = config_dir.join("naal-focus/settings.json");
    let initial = Settings::default();
    config::save(&path, &initial).unwrap();
    let owner = Engine::start(initial.clone(), Some(path.clone()), None);
    wait_until(|| owner.status().safe);

    let bus = zbus::blocking::connection::Builder::session()
        .unwrap()
        .method_timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let dbus = zbus::blocking::Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .unwrap();
    let supervisor_name: String = dbus.call("GetNameOwner", &(CONTROL_SERVICE,)).unwrap();
    let launch = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_naal-focus"));
        command
            .arg("--settings")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &config_dir);
        for name in [
            "WAYLAND_SOCKET",
            "X_PRIVILEGED_WAYLAND_SOCKET",
            "COSMIC_SINGLE_INSTANCE",
            "XDG_ACTIVATION_TOKEN",
            "DESKTOP_STARTUP_ID",
        ] {
            command.env_remove(name);
        }
        ManagedChild(command.spawn().unwrap())
    };

    // Start both before either registers its activation object.
    let mut first = launch();
    let mut second = launch();
    let mut window_pid = 0u32;
    wait_until(|| {
        if let Ok(pid) = dbus.call::<_, _, u32>("GetConnectionUnixProcessID", &(config::APP_ID,)) {
            window_pid = pid;
            true
        } else {
            false
        }
    });
    let (window, duplicate) = if window_pid == first.0.id() {
        (&mut first, &mut second)
    } else {
        assert_eq!(window_pid, second.0.id());
        (&mut second, &mut first)
    };
    wait_until(|| duplicate.0.try_wait().unwrap().is_some());
    assert!(
        duplicate.0.wait().unwrap().success(),
        "duplicate launch must activate the winner and exit"
    );
    assert!(window.0.try_wait().unwrap().is_none());
    assert!(Singleton::acquire("settings-window").is_err());

    let mut reopen = launch();
    wait_until(|| reopen.0.try_wait().unwrap().is_some());
    assert!(reopen.0.wait().unwrap().success());
    let active_pid: u32 = dbus
        .call("GetConnectionUnixProcessID", &(config::APP_ID,))
        .unwrap();
    assert_eq!(
        active_pid, window_pid,
        "reopening must keep the existing process"
    );

    // Shared saves keep recurrence paused. The window's client does not elect
    // another supervisor, and closing that process must leave the owner intact.
    let saved = Settings {
        interval_secs: 600,
        ..initial
    };
    owner.request(Request::Settings(saved.clone()));
    wait_until(|| config::load(&path).is_ok_and(|settings| settings == saved));
    assert_eq!(
        dbus.call::<_, _, String>("GetNameOwner", &(CONTROL_SERVICE,))
            .unwrap(),
        supervisor_name
    );
    window.0.kill().unwrap();
    window.0.wait().unwrap();
    wait_until(|| Singleton::acquire("settings-window").is_ok());
    assert_eq!(
        dbus.call::<_, _, String>("GetNameOwner", &(CONTROL_SERVICE,))
            .unwrap(),
        supervisor_name
    );
    owner.request(Request::Snooze(Duration::from_secs(900)));
    wait_until(|| owner.status().snoozed_secs.is_some());
    assert!(!owner.status().settings.enabled);
    assert!(!owner.status().running);
    assert_eq!(owner.status().settings, saved);

    // Closing frees the window lock so a subsequent launch can create a new UI.
    let mut replacement = launch();
    wait_until(|| {
        dbus.call::<_, _, u32>("GetConnectionUnixProcessID", &(config::APP_ID,))
            .is_ok_and(|pid| pid == replacement.0.id())
    });
    replacement.0.kill().unwrap();
    replacement.0.wait().unwrap();
    drop(owner);
    wait_until(|| Singleton::acquire("applet").is_ok());
    fs::remove_dir_all(dir).unwrap();
}
