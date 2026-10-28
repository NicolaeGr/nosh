# nosh Rust rewrite spec

This folder is the reverse-engineering and implementation spec for rewriting nosh in Rust. It covers the bar, workspaces, system widgets, quick settings, notifications, app launcher, change indicator, idle inhibitor, and the command line. The Vala tree under `src/` is the reference implementation. Every behavior called out here must either survive the rewrite or be replaced by an explicit decision.

## Target shape

One Cargo workspace with two crates:

```text
nosh/
  Cargo.toml                 workspace
  crates/
    nosh-core/               system state, D-Bus, Hyprland, persistence, no GTK
    nosh-ui/                 GTK4 widgets, layer-shell windows, CSS, binary
  docs/
```

`nosh-core` owns everything that talks to the system. `nosh-ui` owns everything that draws. The dependency direction is one way: `nosh-ui` depends on `nosh-core`, never the reverse. `nosh-core` must build and test without GTK, a display server, or a compositor.

The binary is produced by `nosh-ui` and is still called `nosh`.

## Read this when

- Read [architecture.md](architecture.md) first. It defines the crate split, the runtime model, the event and command bridge, window rules, config paths, and the build.
- Read [backends.md](backends.md) before implementing a feature that has more than one possible implementation. It defines the try tree and the fallback rules.
- Read [astal-coverage.md](astal-coverage.md) before deciding whether an Astal library needs a Rust replacement.
- Read [astal-decisions.md](astal-decisions.md) before implementing a backend, changing a timeout, or keeping an old workaround.
- Read the matching file under [features/](features/) while implementing that feature.

Each feature file has four parts:

1. Current behavior, taken from the Vala source.
2. Core work, meaning `nosh-core`.
3. UI work, meaning `nosh-ui`.
4. Definition of done.

Feature files:

- [Bar](features/bar.md)
- [Workspaces](features/workspaces.md)
- [Monitors and layout](features/monitors.md)
- [System stats](features/system-stats.md)
- [Media](features/media.md)
- [Tray](features/tray.md)
- [Battery](features/battery.md)
- [Network and Wi-Fi](features/network.md)
- [Audio](features/audio.md)
- [Brightness](features/brightness.md)
- [Quick settings](features/quick-settings.md)
- [Bluetooth](features/bluetooth.md)
- [KDE Connect](features/kde-connect.md)
- [Notifications](features/notifications.md)
- [App launcher](features/app-launcher.md)
- [Menus](features/menus.md)
- [Custom entries, applets, and scripts](features/custom-applets.md)
- [Authentication](features/auth.md)
- [Lock screen](features/lock-screen.md)
- [Power menu](features/power-menu.md)
- [Idle inhibitor](features/idle-inhibitor.md)
- [Change indicator](features/change-indicator.md)
- [Power profiles](features/power-profiles.md)
- [Command line](features/command-line.md)

## Non-negotiables

These values appear in compositor rules, keybinds, user configs, or the public command line. Keep them unless a decision file says otherwise.

| Item                       | Value                                                                                                                                                 |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Stable application id      | `com.nicolaegr.nosh`                                                                                                                                  |
| Dev application id         | `com.nicolaegr.nosh.dev`                                                                                                                              |
| Bar namespace              | `nosh-topbar`                                                                                                                                         |
| Quick settings namespace   | `nosh-quick-settings`                                                                                                                                 |
| Notifications namespace    | `nosh-notifications-outer`                                                                                                                            |
| App launcher namespace     | `nosh-app-launcher`                                                                                                                                   |
| Change indicator namespace | `nosh-change-indicator`                                                                                                                               |
| App usage history          | `~/.local/share/nosh/app_usage.json`                                                                                                                  |
| App launcher config        | `~/.config/nosh/app_launcher.toml`                                                                                                                    |
| Menu config                | `~/.config/nosh/menus.toml`                                                                                                                           |
| CLI commands               | `volume-up`, `volume-down`, `brightness-up`, `brightness-down`, `brightness-dim`, `brightness-restore`, `menu <id>`, `monitor-cycle`, `power-profile` |

The CSS class names are also public. Users target them in their own stylesheets. Keep the names listed in each feature file.

## Reference

The [reference](reference/README.md) folder contains original pseudocode summaries of the protocols and interfaces the shell uses. It is not copied implementation source. Read it when you need exact D-Bus, Hyprland, Wayland, GTK, or build shapes.

- [D-Bus interfaces](reference/dbus-interfaces.md)
- [Hyprland IPC](reference/hyprland-ipc.md)
- [Wayland protocols](reference/wayland-protocols.md)
- [GTK widgets](reference/gtk-widgets.md)
- [Build and packaging](reference/build.md)

## Implementation order

Do not start with the widgets. The bar depends on services that must be correct before the UI can be thin.

1. Workspace and crate skeleton, error type, logging, GSettings schema, config paths, channel bridge.
2. Monitor service and Hyprland client, including config provider and version detection.
3. Workspace adapters in try-tree order: Hyprland, another compositor-specific adapter, then generic `ext-workspace-v1`.
4. Audio and brightness services, including the brightness actor, DDC/CI, and snapshot, dim, and restore.
5. Battery, network, Bluetooth, UPower power profiles, MPRIS, and tray services.
6. Notification daemon, notification store, and placement rules.
7. Menu engine core: TOML config, builtin sources, external NDJSON sources, search, actions, and the service. Add the `apps` builtin on top of desktop entries.
8. PAM authentication for the power menu or user switch.
9. GTK windows: one bar per monitor, workspaces, system stats, media, tray, battery, Wi-Fi, volume, quick settings button.
10. Quick settings window and dropdowns, including power profiles and Bluetooth discovery.
11. Notifications UI with farthest-monitor placement.
12. Menu UI, including `nosh menu apps`, the wallpaper picker, and custom applets.
13. Change indicator and the idle inhibitor.
14. Replace the old build, then remove the Vala tree.

Each step ends with a testable core behavior before its UI exists. The UI should only render snapshots and send commands.

## What the rewrite changes

The rewrite is not a line-by-line translation. Several Vala patterns exist because of GObject or because the code grew around Astal. Replace them:

- GObject signals become typed events on a channel.
- Property bindings become snapshot updates in the UI. Use `glib::Binding` only for widget-local state.
- Vala `HashTable` and `Gee.List` become `HashMap`, `BTreeMap`, and `Vec`.
- `GLib.Timeout` in the core becomes `tokio::time`.
- Polling loops become event subscriptions with a reconnect supervisor.
- Features with more than one implementation use a specific-first try tree, not scattered compositor checks. See [backends.md](backends.md).
- `AstalIO.Process` and `AstalIO.Variable` are deprecated in Astal. Do not port them.
- `Astal.Application` is deprecated in Astal. Use `gtk::Application`.
- Astal GTK3 and GTK4 widgets are out of scope. Use `gtk4` and `gtk4-layer-shell` directly.
- Shelling out to `top`, `light`, and `hyprctl` on every update is out of scope. Read `/proc`, use a brightness actor with internal and DDC backends, and use the Hyprland socket.
- Astal notification GSettings is out of scope. Use our own schema and a JSON payload store.
- One bar is out of scope. Create one bar per monitor.
- Hyprland `exec_cmd` is not the default launch path. Use a detached double fork.
- Do not use vala-panel-appmenu or `appmenu-glib-translator`. Implement DBusMenu in Rust.

The two crates make this practical: the core can use async Rust, owned state, and real tests, while the UI keeps the small amount of GTK ownership that GTK requires.

## Decisions

These are accepted and reflected in the feature files. Change one only with a note in `astal-decisions.md`.

| Topic                 | Decision                                                                                     | File                                                     |
| --------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Hyprland dispatchers  | Detect config provider and version, support Lua and hyprlang                                 | [features/workspaces.md](features/workspaces.md)         |
| Workspaces            | Hyprland first, then another compositor adapter, then `ext-workspace-v1`                     | [features/workspaces.md](features/workspaces.md)         |
| Special workspaces    | Never show them                                                                              | [features/workspaces.md](features/workspaces.md)         |
| Monitor layout        | One bar per monitor, current and farthest placement rules                                    | [features/monitors.md](features/monitors.md)             |
| Audio backend         | Pulse API through `libpulse-binding`, with `wpctl` fallback                                  | [features/audio.md](features/audio.md)                   |
| Audio maximum         | 1.5 in core and slider, bar scroll stops at 1.0                                              | [features/audio.md](features/audio.md)                   |
| Brightness backend    | Actor with sysfs/logind, DDC/CI, and command backends                                        | [features/brightness.md](features/brightness.md)         |
| Brightness snapshot   | Built-in save, dim, and restore                                                              | [features/brightness.md](features/brightness.md)         |
| Network backend       | NetworkManager D-Bus through `zbus`                                                          | [features/network.md](features/network.md)               |
| Bluetooth backend     | BlueZ D-Bus through `zbus` or `bluez-async`                                                  | [features/bluetooth.md](features/bluetooth.md)           |
| Bluetooth discovery   | Discovery section, scan on dropdown open                                                     | [features/bluetooth.md](features/bluetooth.md)           |
| Tray menus            | Rust DBusMenu client, no vala-panel-appmenu                                                  | [features/tray.md](features/tray.md)                     |
| Notification daemon   | Implement the freedesktop daemon in `nosh-core`                                              | [features/notifications.md](features/notifications.md)   |
| Notification settings | Own GSettings schema, JSON payload store                                                     | [features/notifications.md](features/notifications.md)   |
| App launch            | Detached double fork, not Hyprland `exec_cmd`                                                | [features/app-launcher.md](features/app-launcher.md)     |
| Menus                 | One view plus one source, configured in `menus.toml`; the app launcher is the `apps` builtin | [features/menus.md](features/menus.md)                   |
| Custom applets        | TOML entries, script providers, bar applets                                                  | [features/custom-applets.md](features/custom-applets.md) |
| Power profiles        | Add to quick settings now                                                                    | [features/power-profiles.md](features/power-profiles.md) |
| Auth                  | PAM for power menu, lock screen, user switch                                                 | [features/auth.md](features/auth.md)                     |
| Session lock          | `ext-session-lock-v1` through `gtk4-session-lock`, PAM unlock                                | [features/lock-screen.md](features/lock-screen.md)       |
| App search ranking    | Decayed launch history, recent use outweighs old totals                                      | [features/menus.md](features/menus.md)                   |

## Validation

`nosh-core` must pass:

```sh
cargo test -p nosh-core
```

Use fake backends for D-Bus, Hyprland, sysfs, DDC, and SQLite where possible. The Hyprland parser and dispatch formatter, workspace filter, fuzzy scorer, TOML config, notification serialization, monitor placement score, and brightness gamma mapping are pure functions and need table tests. Add tests for brightness snapshot, dim, and restore, and for the detached spawn helper.

The UI is validated manually against the current shell and with a smoke test under a nested compositor. Layer-shell placement, exclusive zones, keyboard grab, and the visual classes cannot be trusted from unit tests alone.

A rewrite is complete when every command in the non-negotiables table works, every window appears at the same anchor, layer, and namespace, and every feature file's definition of done is checked.
