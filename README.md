# Naal Focus

A COSMIC desktop applet for breathing reminders and optional private reflections.

The [app guide](docs/html/index.html) is the source of truth for behavior, settings, and screenshots.

## Install

Requires a COSMIC Wayland session, Rust 1.93+, Cargo, and `just`. On Pop!_OS/Ubuntu, install the build dependencies:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libfontconfig1-dev fonts-dejavu-core
```

Build and install under `~/.local`:

```sh
just install
```

Add **Naal Focus** through **COSMIC Settings → Desktop → Dock/Panel → Applets**. New settings start paused; updates keep existing preferences. After an update, remove and re-add the applet to load the new binary.

Use `just install /absolute/prefix` for a different install location.

## Uninstall

Remove the applet from the dock/panel first, then run:

```sh
just uninstall
```

Settings and history are kept. If you installed elsewhere, use the same prefix: `just uninstall /absolute/prefix`.

## Development

`just build` builds the debug binaries. See [testing](docs/TESTING.md) for checks and [architecture and safety](docs/ARCHITECTURE.md) for engineering constraints.

Settings live at `$XDG_CONFIG_HOME/naal-focus/settings.json`, falling back to `~/.config/naal-focus/settings.json`.
