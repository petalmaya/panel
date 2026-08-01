# blåShell

<p align="center">
  <img alt="Static Badge" src="https://img.shields.io/badge/debian%20trixe%20-%20?style=for-the-badge&logo=debian&color=purple">
  <img src="https://img.shields.io/github/license/petalmaya/panel?style=for-the-badge&labelColor=101418&color=adabe0" alt="License">
  <br>
  <img src="assets/screenshots/islands_bar_dark.png" alt="blåShell" width="830">
</p>

A batteries-included Wayland panel that replaces your status bar, notification daemon, OSD and more with a single binary. Works out of the box with [Hyprland](https://github.com/hyprwm/Hyprland), [Niri](https://github.com/niri-wm/niri), [Sway](https://github.com/swaywm/sway), and more.

## Credits

This project is a fork and continuation of [VibePanel](https://github.com/prankstr/vibepanel) by [David (prankstr)](https://github.com/prankstr), which was originally licensed under the MIT License. We maintain that license and credit the original author for their work.

## Modifications

* Renamed binary to `blashell`
* App Launcher panel enhancements

## Why BlåShell?

BlåShell is something between a simple status bar and a full desktop shell:

- **Fast & native** – Single Rust binary with GTK4. Direct system integration, low resource usage.
- **Batteries included** – BlåShell replaces several common components with a single binary:
  - **Notifications** – Integrated notification center
  - **OSD** – Built-in on-screen display for volume and brightness
  - **Quick settings** – Native panel for Wi‑Fi, Bluetooth, audio, power profiles and more
- **Minimal config** – Sensible defaults out of the box; customize with TOML, CSS only if needed.
- **Modern aesthetics** – Defaults to a floating "island" design with instant hot‑reloading and features wallpaper adaptive theming that auto‑switches between light and dark.
- **Integrated CLI** – Control volume, brightness, media playback, bar visibility, popovers and idle inhibition.

## Demo

These examples use roughly ~10–35 lines of TOML to get completely different vibes, no CSS required.

https://github.com/user-attachments/assets/fba27921-0886-4e7b-850d-b51341583693

*A few example configurations*

<table align="center">
  <tr>
    <td><a href="assets/screenshots/gruvbox_desktop.png"><img src="assets/screenshots/gruvbox_desktop.png" width="270"></a></td>
    <td><a href="assets/screenshots/frosted_minimal_desktop.png"><img src="assets/screenshots/frosted_minimal_desktop.png" width="270"></a></td>
    <td><a href="assets/screenshots/sonoma_desktop.png"><img src="assets/screenshots/sonoma_desktop.png" width="270"></a></td>
  </tr>
</table>

## Widgets

- **Quick settings**:
  - **Audio** - Control volume and outputs
  - **Brightness** - Adjust screen brightness
  - **Bluetooth** - Manage and pair devices
  - **Wi-Fi** - Connect to and manage networks
  - **VPN** - Connect to NetworkManager-managed VPN connections
  - **Idle Inhibitor** - Toggle idle inhibitor to prevent sleep
- **Workspaces** - clickable indicators with tooltips
- **Window title** - active window with app icon
- **Keyboard layout** - layout indicator with click to cycle
- **Clock** - configurable format with calendar popover
- **Battery** - status with detailed popover and power profiles
- **System tray** - XDG tray support
- **Notifications** - notification center with Do Not Disturb
- **Updates** - package update indicator (dnf, pacman/paru and flatpak support)
- **CPU, Memory, GPU & Network Speed** - system resource monitors (AMD and NVIDIA GPU support)
- **Media** - MPRIS media player controls with album art
- **Custom** - user-defined widgets (scripts, buttons, indicators)
- **Taskbar** - open windows as clickable buttons

## Quickstart

1. Install blaShell:
   
   ***Other distros:** Install runtime dependencies, then:
   
   ```sh
   curl -LO https://github.com/petalmaya/panel/releases/latest/download/blåshell-x86_64-unknown-linux-gnu
   install -Dm755 blåshell-x86_64-unknown-linux-gnu ~/.local/bin/blåshell
   ```
   
   Or build from source.

2. Run it:
   
   ```sh
   blåshell
   ```

See the [Installation wiki](https://github.com/petalmaya/blashell/wiki/Installation) for more information.

## Configuration

Blåshell doesn't require a config file to run, but if you want to customize anything, create a config at `~/.config/blåshell/config.toml`:

```sh
mkdir -p ~/.config/blåshell
blåshell --print-example-config > ~/.config/blåshell/config.toml
```

Here's a minimal example:

```toml
[bar]
size = 32

[widgets]
left = ["workspaces", "window_title"]
center = ["media"]
right = ["quick_settings", "battery", "clock", "notifications"]

[theme]
mode = "dark"
accent = "#adabe0"
```

Changes hot-reload instantly. See the [Configuration wiki](https://github.com/prankstr/vibepanel/wiki/Configuration) for all options.

## Status

BlåShell is based on VibePanel and continues its active development.
Config options and defaults may change between releases.

### Compatibility

- **Compositors:** [Hyprland](https://github.com/hyprwm/Hyprland), [Niri](https://github.com/niri-wm/niri), [Sway](https://github.com/swaywm/sway), [Miracle WM](https://github.com/miracle-wm-org/miracle-wm)
- **Updates widget:** dnf, pacman/paru and flatpak.

## Documentation

Full documentation lives in the [VibePanel wiki](https://github.com/prankstr/vibepanel/wiki):

- [Installation](https://github.com/petalmaya/blashell/wiki/Installation) - Dependencies, building, auto-start
- [Configuration](https://github.com/petalmaya/blashell/wiki/Configuration) - All config options
- [CLI](https://github.com/petalmaya/blashell/wiki/CLI) - Command reference
- [Widgets](https://github.com/petalmaya/blashell/wiki/Widgets) - Widget reference and per-widget options
- [Theming](https://github.com/petalmaya/blashell/wiki/Theming) - Custom CSS styling
- [CSS Variables](https://github.com/petalmaya/blashell/wiki/CSS-Variables) - Full CSS variable reference

## Contributing

Pull requests welcome. Please ensure changes are well-tested.

## License

MIT License - See LICENSE file for details.

### Attribution

This project is derived from [VibePanel](https://github.com/prankstr/vibepanel) by David (prankstr), originally licensed under the MIT License. We maintain the same license and gratefully acknowledge the original author's work.
