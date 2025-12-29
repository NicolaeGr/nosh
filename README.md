# NOSH

This is my try at implementing a shell around Hyprland in astal. For starters this only includes a top bar an a tentative at a quick settings menu.

## Features i want to implemet

- [x] TopBar
- [ ] Quick Settings
- [x] App Launcher
- [ ] Notification Widgets
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

- **500px max width** with **16:10 aspect ratio**
- **Search bar** with fuzzy search support
- **2-column app grid** displaying app icons and names
- **Keyboard navigation**: Enter to launch, Tab/Shift+Tab to navigate
- **SQLite database** tracking app usage frequency
- **Smart sorting**: Most frequently used apps appear first
- **TOML config support** for custom entries
- **In-code entries** with lambda support

### Usage

Trigger the app launcher with:
```sh
nosh app-launcher
```

You can bind this to a key in your window manager (e.g., Hyprland):

```
bind = $mainMod, D, exec, nosh app-launcher
```

### Custom Entries

Create a config file at `~/.config/nosh/app_launcher.toml`:

```toml
[[entry]]
name = "My Custom App"
icon = "application-x-executable"
exec = "/path/to/my/app"

[[entry]]
name = "Terminal in Home"
icon = "utilities-terminal"
exec = "alacritty --working-directory ~"
```

### Data Storage

- App usage data is stored in `~/.local/share/nosh/app_launcher.db`
- Custom config is read from `~/.config/nosh/app_launcher.toml`
