# NOSH

This is my try at implementing a shell around Hyprland in astal. For starters this only includes a top bar an a tentative at a quick settings menu.

## Features i want to implemet

- [x] TopBar
- [ ] Quick Settings
- [ ] App Launcher
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
