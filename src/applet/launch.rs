//! Desktop launch routing and race-free settings-window activation.
use crate::{config, process::Singleton};
use std::{
    collections::HashMap,
    error::Error,
    process::Command,
    time::{Duration, Instant},
};

/// Serialize startup as well as the window lifetime. libcosmic's D-Bus probe
/// alone can race when two panels launch settings before either registers.
pub fn run_settings() -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Singleton::acquire("settings-window") {
            Ok(_window_lock) => {
                cosmic::app::run_single_instance::<super::App>(
                    cosmic::app::Settings::default()
                        .size(cosmic::iced::Size::new(540.0, 760.0))
                        .size_limits(
                            cosmic::iced::Limits::NONE
                                .min_width(440.0)
                                .min_height(360.0),
                        )
                        .exit_on_close(true),
                    super::Flags { standalone: true },
                )?;
                return Ok(());
            }
            Err(error) if error == "Another Naal Focus settings-window is already running." => {
                if activate_existing().is_ok() {
                    return Ok(());
                }
                // The winner may still be creating its window/D-Bus object.
                // Never create a second window if activation fails.
                if Instant::now() >= deadline {
                    return Err("The settings window is already running but could not be activated. Try again.".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn activate_existing() -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(Duration::from_millis(500))
        .build()?;
    let path = format!("/{}", config::APP_ID.replace('.', "/"));
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        config::APP_ID,
        path,
        "org.freedesktop.DbusActivation",
    )?;
    let mut data = HashMap::<&str, zbus::zvariant::Value<'_>>::new();
    if let Ok(token) = std::env::var("XDG_ACTIVATION_TOKEN") {
        data.insert("activation-token", token.into());
    }
    if let Ok(id) = std::env::var("DESKTOP_STARTUP_ID") {
        data.insert("desktop-startup-id", id.into());
    }
    proxy.call("Activate", &(data,))
}

/// Launch the same binary detached; panel shutdown must not kill its window.
pub(super) fn settings_command(token: Option<&str>) -> Result<Command, String> {
    Ok(command_for(
        &std::env::current_exe().map_err(|error| error.to_string())?,
        &crate::process::desktop_socket_path()?,
        token,
    ))
}

fn command_for(
    executable: &std::path::Path,
    desktop_socket: &std::path::Path,
    token: Option<&str>,
) -> Command {
    let mut command = Command::new(executable);
    command.arg("--settings");
    command.env("WAYLAND_DISPLAY", desktop_socket);
    for name in [
        "WAYLAND_SOCKET",
        "X_PRIVILEGED_WAYLAND_SOCKET",
        "COSMIC_SCALE",
        "COSMIC_SINGLE_INSTANCE",
        "XDG_ACTIVATION_TOKEN",
        "DESKTOP_STARTUP_ID",
    ] {
        command.env_remove(name);
    }
    if let Some(token) = token {
        command.env("XDG_ACTIVATION_TOKEN", token);
        command.env("DESKTOP_STARTUP_ID", token);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn command_routes_to_desktop_and_removes_nested_panel_environment() {
        let executable = std::path::Path::new("/opt/naal-focus");
        let desktop_socket = std::path::Path::new("/run/user/1000/wayland-test");
        let command = command_for(executable, desktop_socket, Some("fresh-token"));
        assert_eq!(command.get_program(), executable);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("--settings")]
        );
        let env: HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            env[OsStr::new("WAYLAND_DISPLAY")],
            Some(desktop_socket.as_os_str())
        );
        for name in [
            "WAYLAND_SOCKET",
            "X_PRIVILEGED_WAYLAND_SOCKET",
            "COSMIC_SCALE",
            "COSMIC_SINGLE_INSTANCE",
        ] {
            assert_eq!(env[OsStr::new(name)], None);
        }
        for name in ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"] {
            assert_eq!(env[OsStr::new(name)], Some(OsStr::new("fresh-token")));
        }
        let without_token = command_for(executable, desktop_socket, None);
        let env: HashMap<_, _> = without_token.get_envs().collect();
        assert_eq!(env[OsStr::new("XDG_ACTIVATION_TOKEN")], None);
        assert_eq!(env[OsStr::new("DESKTOP_STARTUP_ID")], None);
    }
}
