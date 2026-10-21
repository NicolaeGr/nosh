# Astal coverage

This file records which Astal libraries the current shell uses, which ones it does not, how large they are, and whether the Rust rewrite needs them. Line counts are rough source lines from `astal-source/lib`, excluding build files, generated files, and most CLI code unless noted. Treat them as a sizing signal, not an exact metric.

The rewrite does not link Astal. These numbers describe the amount of behavior that would need a Rust replacement if a feature is kept.

## Summary table

| Library | Used by nosh | Rough size | Complexity | Need now | Reason |
| --- | --- | --- | --- | --- | --- |
| `hyprland` | Yes, workspaces and urgent | 740 Vala LOC | High | Yes | Core compositor state and dispatch |
| `network` | Yes, bar Wi-Fi and quick settings | 580 Vala LOC | High | Yes | Wi-Fi list, connect, scan, enable |
| `bluetooth` | Yes, quick settings | 680 Vala LOC plus D-Bus | Medium | Yes | Adapter and device control |
| `wireplumber` | Yes, volume, microphone, OSD | 4,900 C LOC plus headers | High | Yes, narrow replacement | Default sink/source and volume |
| `mpris` | Yes, one label in the bar | 1,600 Vala LOC | Medium-high | Yes, subset | Player metadata and controls |
| `notifd` | Yes, daemon and cards | 1,690 Vala LOC | High | Yes | Notification daemon and store |
| `tray` | Yes, bar tray | 950 Vala LOC | High | Yes | StatusNotifier host and a Rust DBusMenu client |
| `battery` | Yes, bar battery | 590 Vala LOC | Low-medium | Yes, small subset | UPower display device |
| `astal4` (GTK4) | Only `Astal.Window` | 1,000 Vala LOC | Low for the window part | No | Replace with `gtk4-layer-shell` |
| `astal3` (GTK3) | No | 1,100 Vala LOC plus C | Medium | No | Wrong toolkit |
| `astal-io` | No, but listed in Meson | 800 Vala LOC | Low, deprecated | No | Deprecated by Astal |
| `apps` | Yes, own implementation | 560 Vala LOC reference | Medium | Yes | App index, search, launch, custom entries |
| `brightness` | Yes, own implementation | 500 Vala LOC reference | Medium | Yes | Internal and DDC/CI brightness, snapshot and restore |
| `powerprofiles` | Yes | 224 Vala LOC | Low | Yes | Quick settings power mode |
| `workspace` | Yes, fallback backend | 880 Vala LOC plus protocol | High | Yes | Specific-first, with generic `ext-workspace-v1` as fallback |
| `idle-notify` | No | 130 Vala LOC plus protocol | Low | No, future | Idle detection for lock or screen blank |
| `cava` | No | 690 C LOC plus header | Medium | No, future | Audio visualizer |
| `auth` | No | 680 C LOC | Medium | Yes | PAM password check for the power menu, lock screen, or user switch |
| `greet` | No | 560 Vala LOC | Medium-high | No, future | greetd greeter |
| `river` | No | 490 Vala LOC plus protocol | Medium | No | Wrong compositor |
| `quarrel` | No | 700 Vala LOC | Low | No | Rust has `clap` |
| `wl` | Indirect only | 850 Vala LOC plus C | Medium | No | Use `wayland-client` directly |
| `wl4` | No, reference | 110 Vala LOC | Low | No, copy the matching logic | Match GDK monitors to Wayland outputs |

## Libraries in use

### hyprland

`AstalHyprland` owns the socket connection, monitors, workspaces, clients, focused state, and an event signal. nosh uses:

- `get_default()`, `workspaces`, `focused_workspace`.
- `notify["workspaces"]`, `notify["focused-workspace"]`.
- `urgent` and `Client.workspace`.
- `message_async` for the Lua focus dispatch.

The Rust core needs a smaller but more correct client: socket2 event reading, initial `j/monitors`, `j/workspaces`, `j/clients`, `j/activeworkspace`, `j/activewindow`, and `j/status`, event parsing, dispatch formatting for Lua and hyprlang, and reconnect on EOF or missing socket. `features/workspaces.md` contains the model and the event list.

The Vala implementation has several null and EOF bugs recorded in `astal-decisions.md`. Do not port those.

### network

`AstalNetwork` wraps libnm. nosh uses:

- `Network.get_default()`, `network.wifi`.
- `Wifi.enabled`, `ssid`, `icon_name`, `active_access_point`, `access_points`, `scan`, `deactivate_connection`.
- `AccessPoint.ssid`, `strength`, `activate`.

The Rust rewrite uses NetworkManager D-Bus directly. Implement the fixes that are still open upstream:

- Group access points by SSID, mode, and security.
- Detect security type and create the right connection setting.
- Handle a null BSSID.
- Refresh devices on hot-plug.
- Add VPN support as a separate service if the quick settings VPN toggle becomes real.

`features/network.md` specifies the model and the UI.

### bluetooth

`AstalBluetooth` uses a BlueZ object manager. nosh uses `is_powered`, `devices`, `paired`, `connected`, `name`, `icon`, `connect_device`, `disconnect_device`.

The Rust rewrite needs the adapter state machine from open PR #479, device battery from `org.bluez.Battery1`, and an icon name derived from the BlueZ icon. The current code does not pair devices and only lists paired devices. Keep that scope for v1.

### wireplumber

`AstalWp` is a C wrapper over libwireplumber. nosh uses a very small part:

- `audio.default_speaker`, `default_microphone`.
- `volume`, `volume_icon`.
- `notify["volume"]`.

Reimplementing the full library in Rust is unnecessary. The core needs a `AudioService` with one default output and one default input, volume in the range `0.0..=1.5`, mute, icon name, and change events. Use the Pulse API or `wpctl`, as described in `features/audio.md`. The rest of AstalWp, including routes, profiles, channels, video, and per-application streams, is not needed for this shell.

### mpris

nosh uses one player and two fields: `artist` and `title`. The Astal library has 1,600 lines because it handles every MPRIS property, position polling, loop and shuffle, metadata parsing, and cover art caching.

The Rust core should implement the MPRIS subset now:

- Track `org.mpris.MediaPlayer2.*` names through `NameOwnerChanged`.
- Read `Identity`, `PlaybackStatus`, `Metadata`, `CanControl`, `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`.
- Expose title, artist, album, art URL, length, position, and playback status.
- Implement play, pause, play_pause, next, previous, stop, and seek if the bar gains controls.
- Cache cover art if the UI displays it.

Do not port loop, shuffle, rate, tracklist, or lyrics until a feature needs them.

### notifd

`AstalNotifd` is both a daemon and a client. nosh uses it as the daemon and as the source of the notification list, actions, summary, body, app icon, image, and dismiss.

The Rust core must implement the freedesktop notification server. This is the largest core component. `features/notifications.md` contains the D-Bus surface, the hints, the timeout rules, the persistence format, and the UI behavior.

### tray

`AstalTray` implements the StatusNotifierWatcher host, the item proxy, and a DBusMenu importer through `appmenu-glib-translator`. nosh uses `item_added`, `item_removed`, `get_item`, `gicon`, `tooltip_markup`, `menu_model`, `action_group`, and `insert_action_group`.

The Rust rewrite needs a tray host and a menu model. Do not use `appmenu-glib-translator` from vala-panel-appmenu. Implement the DBusMenu client in Rust with `zbus`, as described in `features/tray.md`. This removes the C dependency and keeps the menu behavior in one language.

### battery

nosh uses `get_default()`, `battery_icon_name`, `percentage`, and `notify["percentage"]`.

The Rust core only needs the UPower `DisplayDevice` and the icon rule from `AstalBattery.Device.sync()`:

```text
if not a battery: "battery-missing-symbolic"
else if percentage >= 0.95 and charging: "battery-level-100-charged-symbolic"
else: "battery-level-{round(percentage * 10) * 10}{-charging}-symbolic"
```

UPower returns percentage in `0..100`. Astal normalizes it to `0..1`. Keep the normalized value inside the core and format at the edges.

## Libraries replaced or not used

### `astal4` and `astal3` GTK widgets

The user interface will use plain GTK4. Do not port `Astal.Application`, `Astal.Window`, `Astal.Box`, `Astal.Slider`, `Gizmo`, or any GTK3 widget.

The one behavior to keep from `Astal.Window` is the layer-shell setup: anchors, exclusivity, layer, keyboard mode, namespace, margins, and the initial 1x1 size request. Use `gtk4-layer-shell`.

`Astal.Application` is deprecated and nosh does not use it. Use `gtk::Application`.

### `astal-io`

`AstalIO.Process`, `AstalIO.Time`, `AstalIO.Variable`, and `AstalIO.Daemon` are marked deprecated in the Astal source. nosh lists `astal-io-0.1` in Meson but no source uses it. Do not port any of it. Use `std::process`, `tokio::time`, and `glib::timeout_add_local`.

### `apps`

`AstalApps` provides desktop entry enumeration, a fuzzy score, and a frequency cache in `~/.cache/astal/apps-frequents.json`. The app launcher is required. The rewrite implements its own index, SQLite usage table, TOML config, script providers, and plugins. `AstalApps` is a reference for a later search improvement, not a dependency. Adopting it now would change ranking and storage.

### `brightness`

`AstalBrightness` reads `/sys/class/backlight` and `/sys/class/leds`, watches files, guesses the main screen and keyboard, and writes through logind or udev. The rewrite implements its own brightness service because it also needs DDC/CI external monitors, command backends, and save, dim, and restore.

Use AstalBrightness as the reference for internal device discovery and the logind write. The Rust service is larger than AstalBrightness because of DDC and the actor model, not smaller.

### `powerprofiles`

`AstalPowerProfiles` is 224 lines over the `org.freedesktop.UPower.PowerProfiles` D-Bus API. The rewrite uses it now. Add a power mode control to quick settings with `power-saver`, `balanced`, and `performance`. See `features/power-profiles.md`.

### `workspace`

`AstalWorkspace` implements `ext-workspace-v1` with managers, groups, per-monitor views, and autocommit. It is 880 lines plus a Wayland protocol. The rewrite uses a try tree: Hyprland IPC first, then another compositor-specific adapter, then the generic `ext-workspace-v1` backend. Specific adapters are preferred because they expose urgent and special workspace semantics exactly. Special workspaces are never shown. See `features/workspaces.md` and `backends.md`.

This is the right abstraction for per-monitor bars when a specific adapter is not available. A workspace group maps to an output, so each bar asks the backend for its group. If the compositor supports neither a specific adapter nor the protocol, the bar has no workspaces.

### `idle-notify`

`AstalIdleNotify` implements `ext-idle-notify-v1` and is about 130 lines plus a protocol. It reports when a seat has been idle for a timeout. nosh does not use it. nosh implements the opposite feature: an idle inhibitor that prevents idle.

Do not replace the inhibitor with this library. They solve different problems. Add idle detection only if the shell gains auto-lock, screen blanking, or a hide-when-idle bar. If that happens, share one Wayland connection between the inhibitor and the notifier.

### `cava`

`AstalCava` wraps libcava and exposes bars, values, framerate, input, and source. It is 690 C lines plus a header. nosh does not use it. It is a future audio visualizer for the planned central console or bar. Not needed now.

What it entails: link libcava or reimplement its FFT and PipeWire capture, expose bars and values, and draw them with a custom GTK widget. The library is not the hard part; the visual design and the real-time drawing are.

### `auth`

`AstalAuth` wraps PAM and is about 680 C lines. The rewrite adds it now for the power menu, user switch, and a future lock screen. Use a Rust PAM binding or a small `libpam` FFI layer. Keep the password in-process, zeroize it, and never log it. See `features/auth.md`.

### `greet`

`AstalGreet` implements the greetd IPC protocol and a greeter flow. It is about 560 Vala lines. It is a login greeter, not a shell feature. It would entail a full-screen greeter UI, a user and session picker, PAM through greetd, and starting the compositor session after authentication. It duplicates a display manager. Not needed now.

### `river`

`AstalRiver` implements the river status and control protocols and is about 490 Vala lines plus protocol code. nosh targets Hyprland. Not needed now. It is only relevant if the shell becomes compositor-neutral and wants river-specific control.

### `quarrel`

`AstalQuarrel` is a CLI argument parser. It is about 700 Vala lines. The Rust rewrite uses `clap` or `argh`. Do not port it.

### `wl` and `wl4`

`AstalWl` wraps `wl_display`, `wl_registry`, `wl_output`, and `wl_seat` for libraries that need Wayland protocols. It is about 850 Vala lines plus C. nosh does not link it directly. The idle inhibitor uses the raw `Wl` namespace from `src/utils/Zwp`.

The Rust core uses `wayland-client` and `wayland-protocols` for the idle inhibitor, generic workspaces, and monitor outputs. `AstalWl4` matches a `Gdk.Monitor` to an `AstalWl.Output`. The rewrite needs that matching for per-monitor bars, so copy the state machine logic from `lib/wl/wl4/src/match.vala` even though the library is not used.

## Dead dependencies in the Vala build

The Meson file lists `astal-io-0.1` and `astal-power-profiles-0.1`. `astal-io` is unused and deprecated. Remove it. `astal-power-profiles` is now used through the rewrite's own D-Bus service, not as an Astal dependency.

`libnm` is only present because `AstalNetwork` links it. The Rust D-Bus implementation does not need libnm. `gee-0.8` is only present because the Vala code uses `Gee.ArrayList`. The Rust code uses `Vec`. `sqlite3` stays for the app usage database. `gtk4-layer-shell-0` stays for the windows.

## Definition of done

- Every used library in the summary table has a Rust service or an explicit feature-level replacement.
- Every unused library has a "not needed now" entry with a future trigger.
- No Astal, GObject, or libnm symbols appear in the Rust dependency graph.
- The dead Meson dependencies do not reappear as unused Rust dependencies.
