# NOSH

This is my try at implementing a shell around Hyprland in astal. For starters this only includes a top bar an a tentative at a quick settings menu.

## Features i want to implemet

- [x] TopBar
- [x] Quick Settings
- [x] App Launcher
- [x] Notification Widgets
- [x] System Controlls Indicator
- [ ] Central Console for media and notes
- [ ] PowerMenu / User Switcher

## Dependencies

- vala
- meson
- blueprint-compiler
- sass
- astal4
- astal-battery
- astal-wireplumber
- astak-network
- astal-mpris
- astak-power-profiles
- astal-tray
- astal-bluetooth
- sqlite3
- libgee-0.8

## How to use

- developing

  ```sh
  meson setup build --wipe --prefix "$(pwd)/result"
  meson install -C build
  ./result/bin/simple-bar
  ```

- installing

  ```sh
  meson setup build --wipe
  meson install -C build
  simple-bar
  ```

- adding new vala files will also have to be listed in `meson.build`
- adding new scss files requires no additional steps as long as they are imported from `style.scss`
- adding new ui (blueprint) files will also have to be listed in `meson.build` and in `gresource.xml`

## App Launcher

The app launcher is a rofi replacement with the following features:

- **Search bar** with fuzzy search support
- **2-column app grid** displaying app icons and names
- **SQLite database** tracking app usage frequency
- **Smart sorting**: Most frequently used apps appear first
- **TOML config support** for custom entries
- **In-code entries** with lambda support

### Usage

Trigger the app launcher with:

```sh
nosh app-launcher
```

### Custom Entries

On first launch, an example config file will be automatically created at `~/.config/nosh/app_launcher.toml`.

To use the config file, remove or comment out the first line (`# EXAMPLE_CONFIG`), then add your custom entries:

```toml
[[entry]]
name = "Custom App"
icon = "application-x-executable"
exec = "/path/to/executable"

[[entry]]
name = "Firefox (Work Profile)"
icon = "utilities-terminal"
exec = "firefox -P work"
```

### Data Storage

- App usage data is stored in `~/.local/share/nosh/app_launcher.db`
- Custom config is read from `~/.config/nosh/app_launcher.toml`

## System controlls

The system controlls module provides an unified way to manage system settings while indicating their updated status in a floating widget on the bottom of the screen.

### Available controlls

- Brightness
- Volume
