# Naal Focus

Desktop breathing reminders, with a full COSMIC applet and an experimental native Mac prototype.

The [app guide](docs/html/index.html) is the source of truth for behavior, settings, and screenshots.

## Linux (COSMIC)

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

## macOS prototype

For Apple Silicon Macs running macOS 14+. Includes menu-bar controls, multi-monitor reminders, breathing, and settings. Reflections, charts, window history, and the dashboard are Coming soon. Live safety testing is still required; see [testing](docs/TESTING.md).

Once the repository and preview release are public, install the prebuilt app without build tools:

```sh
curl -fsSL https://github.com/rinaldo-rex/naal-focus/releases/download/v0.9.0-macos-preview.1/install-macos.sh | sh
```

The installer checks the ZIP checksum and app signature, installs into `~/Applications`, and does not launch the app. No `sudo` or GitHub login is needed. Open **Naal Focus** from that folder when ready.

To build from source instead, install Xcode Command Line Tools (`xcode-select --install`), Rust 1.93+, and `just`, then run:

```sh
just bundle-macos   # target/macos/Naal-Focus-macOS.zip
just install        # ~/Applications/Naal Focus.app; does not launch it
```

Quit the app before updating. Test builds are ad-hoc signed, not notarized; macOS may block opening a downloaded copy. Approve it through macOS only if you trust the build. No Apple account, login item, or tracking permission is needed for this prototype.

The [Mac workflow](.github/workflows/macos.yml) builds and tests on Apple Silicon. Its temporary artifact contains the app ZIP, checksum, installer, and demo settings captures; a published preview release provides the lasting download. CI verifies builds and automated checks, not live reminder safety.

## Uninstall

On Mac, quit the app and move `~/Applications/Naal Focus.app` to the Trash. Settings are kept.

For source installations on either platform, use the command below. On Linux, remove the applet from the dock/panel first; on Mac, quit the app.

```sh
just uninstall
```

Settings and history are kept. If you installed elsewhere, use the same prefix: `just uninstall /absolute/prefix`.

## Development

`just build` builds the debug binaries. See [testing](docs/TESTING.md) for checks and [architecture and safety](docs/ARCHITECTURE.md) for engineering constraints.

Linux settings live at `$XDG_CONFIG_HOME/naal-focus/settings.json`, falling back to `~/.config/naal-focus/settings.json`. Mac preferences live at `~/Library/Application Support/Naal Focus/settings.json`.
