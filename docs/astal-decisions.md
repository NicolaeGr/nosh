# Astal decisions and defects

This file records the places where the current Vala implementation or the vendored Astal source should not be copied as-is. Each entry names the current behavior, the problem, and the Rust behavior. Accepted decisions are marked in the decision log.

The rewrite may break Astal API compatibility. Only user-facing behavior, config formats, paths, namespaces, and command names need migration. Do not preserve an Astal bug or an old workaround just because the Vala code used it.

The rewrite targets current Hyprland, meaning Lua dispatchers, with a hyprlang fallback. That choice is assumed throughout. Confirm it before implementing dispatch.

## Hyprland

### Lua dispatchers and config provider

Current: `AstalHyprland.Client.kill`, `focus`, `move_to`, and `toggle_floating` call hyprlang dispatchers such as `closewindow`, `focuswindow`, `movetoworkspacesilent`, and `togglefloating`. `Workspace.focus` calls `workspace`. `Monitor.focus` calls `focusmonitor`. nosh works around this in `src/bar/widgets/Workspaces.vala` by sending a raw Lua command through `hypr.message_async`:

```text
dispatch hl.dsp.focus({ workspace = "<id>" })
```

Problem: the library API is wrong for a Lua-configured Hyprland, and the UI carries a compositor-specific workaround.

Recommended: in `nosh-core`, read the provider from `hyprctl -j status` (`configProvider` is `"lua"` or `"hyprlang"`). Build dispatch strings through one formatter with two branches. Use the Lua forms from PR #446:

| Action                    | Lua                                                                                     | hyprlang fallback                                      |
| ------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Focus workspace           | `hl.dsp.focus({ workspace = "<id>" })`                                                  | `dispatch workspace <id>`                              |
| Focus client              | `hl.dsp.focus({ window = "address:0x<addr>" })`                                         | `dispatch focuswindow address:0x<addr>`                |
| Focus monitor             | `hl.dsp.focus({ monitor = <id> })`                                                      | `dispatch focusmonitor <id>`                           |
| Close client              | `hl.dsp.window.close({ window = "address:0x<addr>" })`                                  | `dispatch closewindow address:0x<addr>`                |
| Move client to workspace  | `hl.dsp.window.move({ window = "address:0x<addr>", workspace = <id>, follow = false })` | `dispatch movetoworkspacesilent <id>,address:0x<addr>` |
| Toggle floating           | `hl.dsp.window.float({ window = "address:0x<addr>" })`                                  | `dispatch togglefloating address:0x<addr>`             |
| Move workspace to monitor | `hl.dsp.workspace.move({ workspace = <id>, monitor = <monitor> })`                      | `dispatch moveworkspacetomonitor <id> <monitor>`       |
| Move cursor               | `hl.dsp.cursor.move({ x = <x>, y = <y> })`                                              | `dispatch movecursor <x> <y>`                          |
| Execute command           | `hl.dsp.exec_cmd("<command>")`                                                          | `dispatch exec <command>`                              |

Send dispatch over the Hyprland command socket, not through a `hyprctl` subprocess. Serialize requests through one task so replies cannot interleave.

Accepted: support both syntaxes. Detect `configProvider` from `hyprctl -j status`. If it is absent, use `hyprctl -j version` and a version capability table. Re-read both on `configreloaded` and reconnect. Never assume the syntax from a hardcoded version alone.

### Missing event socket and EOF

Current: `AstalHyprland.get_default()` calls `connection("socket2")`, then passes the result to `new DataInputStream(h.socket2.input_stream)` without a null check. If the event socket is missing, this dereferences null or crashes. `watch_socket` also calls `read_line_async.end()` and `handle_event.begin(line, ...)` without checking for a null line at EOF. Both bugs are covered by open PRs #488 and #481.

Recommended: treat a missing socket as a failed connection, return an error, and let the supervisor retry with backoff. On a null line, stop the read loop and reconnect. Do not spin on `read_line_async` after EOF.

### Event parsing nulls and indexing

Current `handle_event` in `hyprland.vala` has several unchecked accesses:

- `monitorremoved` calls `get_monitor_by_name(args[1]).id` without a null check.
- `closewindow` and `destroyworkspacev2` call `.removed()` on a possibly missing object.
- `urgent` calls `get_client(args[1])` and passes a possibly null client.
- The final `event(args[0], args[1])` indexes `args[1]` for every event, including events with no payload such as `configreloaded`.

Recommended: parse events into a typed enum. Validate the argument count before indexing. Treat a missing object as a no-op and log at debug level, not critical.

### Object identity and workspace focus

Current nosh compares `hypr.focused_workspace == ws` and `hypr.focused_workspace.id != ws_id`. Object identity is fragile after a resync because the workspace object can be replaced.

Recommended: compare workspace IDs, not object identity. Keep urgent state keyed by workspace ID in the core model.

### Reconnect loop in the UI

Current `Workspaces` polls every 500 ms when `get_default()` returns null, and reconnects from the widget. The widget also disconnects and reconnects handlers on every sync.

Recommended: the core owns the connection and retries. The UI receives `WorkspaceEvent::Unavailable` and `WorkspaceEvent::Snapshot`. Remove the timer and the manual handler bookkeeping from the widget.

### Special workspace filter

Current filter: `if (ws.id < -99 || ws.id > -2)`. This includes `-1` and `0` and excludes `-99..-2`. Hyprland uses negative IDs for special workspaces.

Decision: never show special workspaces. For Hyprland, show only numeric ids greater than 0 and names that do not start with `special:`. For the generic backend, skip workspaces the compositor marks as special or hidden. Do not add a separate special workspace indicator.

### Groups and inhibiting idle

Open PR #428 adds group support to clients, and PR #435 adds `inhibiting_idle` from the `inhibitingIdle` JSON member. Neither is merged. The current shell does not need groups. `inhibiting_idle` is useful for a smarter idle inhibitor, but the shell's own inhibitor is independent.

Recommended: parse both fields into the model now, even if the UI ignores them. This avoids a second event-parser change later. Do not add UI for groups in the first rewrite.

## Workspaces and monitors

### Generic workspaces

Decision: implement a specific-first try tree. Hyprland IPC comes first, then another compositor-specific adapter, then `ext-workspace-v1`, then the empty backend. Specific adapters are preferred because they expose urgent and special workspace semantics exactly. The generic backend is the fallback and the path for compositors without an adapter. Resolve capabilities separately so a missing move capability does not disable reads or activation. See `backends.md`.

### Special workspaces

Decision: never show special workspaces in the bar. For Hyprland, filter to numeric ids greater than 0 and names that do not start with `special:`. For the generic backend, skip workspaces the compositor marks as special or hidden. Do not show the active special workspace either.

### Per-monitor bars

Decision: one bar per enabled monitor. Each bar sets its layer-shell monitor, uses the same namespace, and filters workspaces by its monitor connector. Rebuild the bar set on monitor add, remove, and layout changes.

### Active windows

Decision: quick settings, the app launcher, and the change indicator open on the current monitor. Use the compositor's focused monitor, then the pointer monitor as a fallback. Set the layer-shell monitor before presenting.

### Notification placement

Decision: notifications appear on the farthest monitor in the configured corner direction. The position comes from GSettings. The rule is not Euclidean distance from the primary; it is the monitor whose requested corner extends furthest in that direction. Recompute on layout changes. See `features/monitors.md`.

### Monitor configuration

Decision: read monitor layout generically, write through a compositor adapter. There is no general Wayland protocol for output configuration across compositors. Use GDK and `wl_output`/`zxdg_output_manager_v1` to read. Use `hyprctl` for Hyprland and `zwlr_output_manager_v1` for wlroots compositors that support it. If no write backend exists, the shell is read-only for layout and still places bars and windows correctly. Presets: extend, mirror, single, panning. See `features/monitors.md`.

## Network

### Access point identity and null BSSID

Current `AstalNetwork.Wifi` keys access points by `ap.bssid` and calls `_access_points.get(ap.bssid)` in `on_active_access_point`. A null BSSID crashes or loses the active access point. Open PR #454 guards the null case.

Recommended: key access points by a stable internal ID that does not require a non-null BSSID. Use BSSID when present, otherwise the object path or SSID plus frequency.

### No grouping and wrong security

Current `AccessPoint.activate` always creates a `wpa-psk` connection and pins the BSSID. It cannot connect to open, WEP, WPA3-SAE, OWE, or enterprise networks. The Wi-Fi dropdown also shows one row per BSSID, so a dual-band router appears twice. Open PR #477 fixes grouping, security detection, and the connection settings.

Recommended: group access points by SSID, mode, and security. Show the strongest AP for the group and expose `access_point_count`. Detect security with `NM.Utils.security_valid` over the ordered type list from PR #477. Create the correct setting for none, OWE, WEP, WPA-PSK, WPA2-PSK, and SAE. Require an existing profile for enterprise networks and say so in the UI.

### No password prompt

The current dropdown calls `ap.activate.begin(null)` for a new secured network. That creates a connection with a null PSK and fails. There is no password dialog.

Recommended: when a secured network has no saved connection, open a password dialog and pass the password to the core. For open networks, activate without a password. For enterprise networks, tell the user to create a profile with `nm-connection-editor` or the desktop settings.

### Hot-plug

Current `AstalNetwork` builds `wifi` and `wired` once at startup. A USB Wi-Fi or Ethernet adapter plugged in later is invisible. Open PR #449 fixes this.

Recommended: watch `DeviceAdded` and `DeviceRemoved` on `org.freedesktop.NetworkManager`, keep stable wrapper objects, and expose an `is_active` flag. The bar Wi-Fi widget hides when no Wi-Fi device is present and reappears after hot-plug.

### VPN

Open PR #478 adds a VPN service over connection profiles, keyed by UUID, covering both `vpn` and `wireguard` types. The current quick settings VPN toggle only flips a local boolean.

Recommended: do not wire the VPN toggle until the VPN service exists. Show it disabled with a tooltip, or remove it from the first rewrite. Add the service later. The PR design is a good model: a profile list, `is_active`, `icon_name`, async activate and deactivate, and an `activation_failed` reason.

## Bluetooth

### Blocking D-Bus calls

Current `Adapter.start_discovery`, `stop_discovery`, `remove_device`, `Device.pair`, `connect_profile`, and `disconnect_profile` are synchronous D-Bus calls. On a slow or unresponsive BlueZ daemon they block the calling thread. Open PR #479 makes them async.

Recommended: all BlueZ methods are async in `nosh-core`. A method call from the UI is a command, and the result is an event or a log line.

### Adapter state

Current `Adapter.powered` is a bool. Toggling power goes through rfkill and can take seconds or fail without a reply. The UI has no "turning on" state. Open PR #479 adds `AdapterState { ABSENT, OFF, ON, TURNING_ON, TURNING_OFF }`, an icon name, and a timeout.

Recommended: model the adapter state machine from the start. Do not show the switch as settled until the adapter reports or the 30 second timeout expires. Keep the timeout as a constant in the core.

### Battery ordering and stale handlers

Current `Bluetooth` handles interfaces in the order delivered by the object manager. A `Battery1` interface can arrive before its `Device1`, so the battery is dropped. Removing a battery leaves the old handler connected. Open PR #479 fixes both.

Recommended: sort interfaces so `Adapter1` and `Device1` are handled before `Battery1`, and store the battery property-change handler so it can be disconnected. Recompute `battery_percentage` on removal.

### Icon names

Current `BluetoothDropdown.get_device_icon` returns the raw BlueZ `icon` property, for example `audio-headset`, without the `-symbolic` suffix. Open PR #479 adds `Device.icon_name` that appends `-symbolic` and falls back to `bluetooth-symbolic`.

Recommended: use `icon_name` and fall back to `bluetooth-symbolic`. Keep the value in the core so the UI does no string manipulation.

### Pairing and discovery

Current UI lists only paired devices and can only connect or disconnect. The refresh button rebuilds the list but does not start discovery. The adapter API supports discovery and pairing.

Decision: add discovery now. Opening the Bluetooth dropdown starts a scan. Show a separate `Available devices` section, stop after 30 seconds, and let the refresh button restart the scan. Pairing from an available device requires a minimal `org.bluez.Agent1` confirmation flow. If the agent is deferred, ship discovery without pairing and do not show a pairing button. See `features/bluetooth.md`.

## Notifications

### Synchronous startup

Current `AstalNotifd.Notifd.construct` uses `Bus.own_name` plus a nested `MainLoop` or a busy `MainContext.iteration` loop to make startup synchronous. This is fragile inside a GTK application.

Recommended: start the notification daemon as an async task. `zbus` can request the bus name and register the object without blocking. The UI starts with an empty notification list and receives a snapshot when the daemon is ready.

### GSettings and persistence

Current settings and persisted notifications live in GSettings schema `io.astal.notifd`. The `notifications` key stores a variant array of serialized notifications. The `server-capabilites` key has a typo in the name.

Decision: use our own GSettings schema, for example `com.nicolaegr.nosh`, for settings such as ignore-timeout, dont-disturb, default-timeout, max-visible, display-timeout, notification position, and monitor policy. Keep notification payloads in `~/.local/state/nosh/notifications.json`, not GSettings. Do not depend on `io.astal.notifd`. On first run, import the old keys once if that schema exists, then ignore it.

### Fixed display timeout

Current `NotificationCard` uses a fixed 5000 ms display timeout. It does not read the notification's `expire_timeout`, the daemon's `default_timeout`, or `dont_disturb`.

Decision: define two timeouts clearly.

- The daemon timeout resolves the notification and emits `NotificationClosed`. It uses `expire_timeout` when positive, otherwise `default_timeout`, and respects `ignore_timeout`.
- The UI display timeout only removes the card. It defaults to 5000 ms, pauses on hover, and calls `NotificationDismiss` when it expires. If `dont_disturb` is on, store the notification but do not create a card.

Also place the notification window on the farthest monitor for the configured corner. This is a new requirement, not current behavior. See `features/monitors.md`.

### Missing notification features

Current UI ignores urgency, sound hints, body images, action icons, resident, transient, and activation tokens. It renders action labels as text buttons and invokes the action ID.

Recommended: keep the v1 scope. Add urgency styling and body images only when a user asks. The daemon must still store all hints and preserve them across restarts.

## Tray

### Zero-sized pixmaps

Current `pixmap_to_pixbuf` calls `Gdk.Pixbuf.from_bytes` with the first or largest pixmap. Some StatusNotifierItem implementations send a zero-sized pixmap, which asserts. Open PR #487 guards this.

Recommended: ignore pixmaps with width or height <= 0 and fall back to `icon_name` or a missing-image icon. Cap decoded pixmap size to avoid a malicious or broken item allocating a huge buffer.

### Blocking menu calls

Current `TrayItem.about_to_show` uses `Bus.get_sync` and `call_sync`. Icon theme lookup recursively walks the filesystem. `Tray` creates a watcher and proxy synchronously.

Recommended: use `zbus` async for `AboutToShow`, menu layout, and event calls. Search the icon theme with a bounded depth and cache the result. Do not block the GTK thread on a tray item that is slow to respond.

### Combined service and object path

Commit `be986c1` fixes the watcher handling of a registration string that contains both a service and an object path. Keep that behavior.

Recommended: parse `RegisterStatusNotifierItem` in this order: if the argument starts with `/`, use the sender as the bus name and the argument as the path; else if it contains `/`, split at the first slash; else use the argument as the bus name and `/StatusNotifierItem` as the path. Watch `NameOwnerChanged` and remove items when the owner disappears.

### Menu model

Current Astal uses `appmenu-glib-translator` to expose a `GMenuModel` and `GActionGroup`. The Rust rewrite can either link the same C library or implement the DBusMenu protocol.

Decision: do not use `appmenu-glib-translator` from vala-panel-appmenu. Implement a DBusMenu client with `zbus` and rebuild a `gio::Menu` on `LayoutUpdated` and `ItemsPropertiesUpdated`. At most, copy the translation logic. `features/tray.md` has the protocol details. This removes the C dependency and keeps the tray implementation in Rust.

## Audio

### Missing external changes

Current `VolumeControl.volume_changed` is emitted only by its own `increase`, `decrease`, and `set` methods. A volume change from another application or keybind updates the speaker but does not show the change indicator.

Recommended: the audio service emits `VolumeChanged` with an `origin` field. `Origin::User` means the shell initiated the change and should show the indicator. `Origin::External` means the value changed elsewhere and only updates widgets. This avoids an OSD storm on startup or when another app adjusts volume.

### Inconsistent maximum

Current `VolumeControl.MAX_VOLUME` is `1.5`, the quick settings slider goes to 150, and the bar scroll handler clamps to `0..1`. Scrolling up in the bar can never reach the amplified range, while the slider can.

Decision: allow `0.0..=1.5` in the core, the quick settings slider, and the OSD. The bar scroll wheel stops at `1.0` so a stray scroll cannot enter amplified range. Scrolling down from above `1.0` still works. Keep one maximum constant in the core and one bar cap.

### Volume icon and mute

Current the bar binds `speaker.volume-icon` and nothing else. There is no mute control and no device selector.

Recommended: keep the volume icon binding for parity. Add a click action for mute and a device selector only as a later feature. The core should still expose `mute` because the backend has it and the OSD may need it.

## Brightness

### Two backends

Current `src/utils/system_control/BrightnessControl.vala` shells out to `brightnessctl --exponent=2.0`, while `src/quick_settings/widgets/BrightnessSlider.vala` shells out to `light`. The slider starts a thread and joins it on every change. The two paths can disagree and both tools must be installed.

Decision: one service with multiple backends behind an actor queue.

- Internal: sysfs plus `org.freedesktop.login1.Session.SetBrightness`, with `brightnessctl --exponent=2.0` as a fallback.
- External: DDC/CI VCP `0x10` through `ddcutil` by default, with an optional `ddc-hi` fast path.
- Command: named command devices for special displays.
- Save, dim, and restore: built-in commands with a JSON snapshot in `~/.cache/nosh/brightness/`.

Remove `light`. The actor coalesces rapid sets and serializes slow DDC calls so the UI never blocks. See `features/brightness.md`.

### Gamma

`brightnessctl --exponent=2.0` applies a gamma curve. The exact mapping is `raw = max * (percent / 100)^2`. If sysfs is used directly, apply the same curve in Rust so brightness feels the same.

Decision: store the normalized UI value `0.0..=1.0`, apply `gamma = 2.0` to get the raw internal value, and expose the display percentage as `brightness * 100`. Do not apply the gamma curve to DDC/CI values; the monitor owns its own scale. Test the internal mapping at 0, 0.1, 0.5, 1.0.

### Keyboard backlight

AstalBrightness supports LEDs and keyboard backlight. nosh only uses the screen slider. Do not implement keyboard backlight until a feature asks for it.

## System stats

Current `SystemStats` runs `top -bn1` every 3 seconds and parses `Cpu(s):` and `Mem`. The output is locale-dependent, the first CPU sample is since boot, and spawning a process every 3 seconds is wasteful.

Recommended: read `/proc/stat` and `/proc/meminfo` in the core. Keep two samples for CPU and compute the busy fraction from the delta. Memory used is `MemTotal - MemAvailable`. Keep the 3 second interval, the four color thresholds, and the label format for parity.

## App launcher

### Launch path

Current `AppEntry.launch_desktop_app` strips `%f`, `%F`, `%u`, `%U`, `%d`, `%D`, `%n`, `%N`, `%v`, `%m`, `%i`, `%c`, `%k` from the command line, then sends it through `hyprctl dispatch hl.dsp.exec_cmd("...")`. `launch_command` does the same for custom commands.

Problem: the launcher depends on Hyprland to spawn applications. It also relies on compositor-side shell parsing and does not report failures.

Decision: launch from `nosh-core` with a detached double-fork helper, not `exec_cmd`. `setsid` alone is not enough to guarantee the child survives a shell crash; the double fork reparents the child to init. Set stdin, stdout, and stderr to `/dev/null`, close inherited descriptors, and apply the current environment. For custom command strings, run `sh -c` only because the user explicitly configured a shell command. Keep `spawn_via = "detached" | "hyprland"` as a config option, defaulting to detached. See `features/app-launcher.md`.

### TOML parser

Current `ConfigManager` parses TOML line by line. It accepts lines starting with `name`, `icon`, or `exec`, splits on the first `=`, and strips surrounding quotes. It does not handle inline comments, arrays, escapes, or nested tables. It uses the first line `# EXAMPLE_CONFIG` as a sentinel that disables the file.

Recommended: use `toml` plus `serde`. Keep the `[[entry]]` schema, the `name`, `icon`, and `exec` fields, the example file, and the `# EXAMPLE_CONFIG` sentinel. Accept existing valid files. Do not silently rewrite the user's file.

### Fuzzy search

Current `AppProvider.fuzzy_match_score` is a custom substring scorer with bonuses and gap penalties, and the UI applies description and category divisors. It is not the AstalApps fuzzy algorithm.

Recommended: port the current scorer exactly for parity and put it behind a `SearchRanker` trait. Keep the score constants in one module with table tests. A later change can swap in a better ranker without touching the UI. Do not use `AstalApps` scoring in the first rewrite because it changes ordering.

### Usage database

Keep the SQLite schema and path:

```sql
CREATE TABLE IF NOT EXISTS app_usage (
    desktop_id TEXT PRIMARY KEY,
    frequency INTEGER DEFAULT 0,
    last_used INTEGER DEFAULT 0
);
```

The `last_used` column is written but not read. Keep writing it for a future recency ranker. Use an upsert that increments `frequency` and updates `last_used`.

### Plugin behavior

Current plugins only match a query and produce one entry. They do not have a settings UI, permissions, or async results.

Recommended: keep the `SearchProvider` trait and the two built-in providers. Match the exact prefixes and the restart query rule.

Also implement the user extension points from `features/custom-applets.md`:

- Rich `[[entry]]` entries with description, keywords, terminal, working directory, and environment.
- Script providers with a prefix and JSON-lines output.
- Bar applets that run a command on an interval and display the output.

Do not build a dynamic plugin runtime or an embedded interpreter.

## Power profiles

Decision: add a `PowerProfilesService` now and a control in quick settings. Use the UPower PowerProfiles D-Bus API. The control shows the active profile and cycles through `power-saver`, `balanced`, and `performance`. It waits for `PropertiesChanged` before showing the new profile. Missing daemon disables the control. See `features/power-profiles.md`.

## Auth

Decision: add PAM authentication for the power menu, lock screen, and user switch. Use a Rust PAM binding or a small `libpam` FFI layer. Keep the password in-process, zeroize every copy the shell owns, and never log it. Answer the first prompt of each kind only, so a second module cannot read the password; refuse a stack that asks twice. Run PAM in a blocking thread. Ship and document a `nosh-auth` PAM service file. The lock screen uses `ext-session-lock-v1` directly, not a layer-shell overlay, so the compositor blocks input while it is active. See `features/auth.md` and `features/lock-screen.md`.

## Command line and build

### Dev service restart

Current `App.vala` runs `systemctl --user stop nosh` before starting a dev instance and `systemctl --user start nosh` when the dev bar closes.

Recommended: keep this behind the `dev` feature. A stable build must not stop or start services. Log a warning instead of a critical if `systemctl` is missing.

### Deprecated Astal IO

The Meson dependencies include `astal-io-0.1`, but nothing uses it. Do not port `AstalIO.Process`, `AstalIO.Time`, `AstalIO.Variable`, or `AstalIO.Daemon`.

## Open pull requests that change the plan

These are open on `Aylur/astal` as of this writing. The ones marked relevant should shape the Rust services even though the rewrite does not link Astal.

| PR   | Title                                                         | Relevance                                                                         |
| ---- | ------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| #446 | fix(hyprland): use new lua dispatchers                        | High. Base the dispatch formatter on it.                                          |
| #488 | fix(hyprland): return null when event socket is missing       | High. Copy the null handling.                                                     |
| #481 | fix(AstalHyprland): stop reading after socket EOF             | High. Copy the EOF handling.                                                      |
| #434 | feat(hyprland)!: nullable getters                             | Medium. `get_monitor`, `get_workspace`, and `get_client` return `Option` in Rust. |
| #435 | feat(hyprland): add `inhibiting_idle` property                | Low. Parse it now, no UI.                                                         |
| #428 | feature(hyprland): full group support                         | Low. Parse group addresses later.                                                 |
| #477 | fix(network)!: group access points and fix security detection | High. Implement grouping and security.                                            |
| #478 | feat(network): add VPN support                                | Medium. Use its design when the VPN toggle becomes real.                          |
| #479 | fix(bluetooth): async D-Bus calls and adapter state           | High. Implement the adapter state machine.                                        |
| #449 | fix(network): network device hot-plugging                     | High. Watch device add and remove.                                                |
| #454 | fix(network): guard against null BSSID                        | High. Do not key solely on BSSID.                                                 |
| #457 | fix(network): clear obsolete wifi properties on disconnect    | Medium. Clear SSID, strength, and AP on disconnect.                               |
| #487 | tray: guard against zero-sized IconPixmap                     | High. Ignore zero-sized pixmaps.                                                  |
| #305 | feat(hyprland): add `mirrorOf` member of `Monitor`            | Medium. Parse mirror state for the monitor layout model.                          |
| #462 | prevent error state when turning a monitor on and off         | Medium. Handle disabled monitors without treating them as failures.               |
| #62  | new lib: clipboard                                            | Low, future. KDE Connect clipboard actions and a clipboard history would use it.  |

| #291, #282, #441, #483 | GTK widget additions | None. The rewrite uses plain GTK. |
| #445 | add .deps files | None for Rust. |
| #432 | deprecate properties in favor of methods | Rejected direction. Astal keeps properties for binding. Rust uses explicit state and events, so the debate does not apply. |

## Decision log

Fill in the status column before implementation. The default is the recommendation in this file.

| Decision                     | Recommendation                                                            | Status   |
| ---------------------------- | ------------------------------------------------------------------------- | -------- |
| Backend resolution           | Specific-first try tree with per-capability fallback                      | Accepted |
| Hyprland dispatchers         | Detect config provider and version, Lua first, hyprlang fallback          | Accepted |
| Special workspaces           | Never show them in the bar                                                | Accepted |
| Workspaces backend           | Hyprland first, then another compositor adapter, then `ext-workspace-v1`  | Accepted |
| Audio backend                | Pulse API, `wpctl` fallback                                               | Accepted |
| Audio maximum                | 1.5 in the core and slider, bar scroll stops at 1.0                       | Accepted |
| Brightness backend           | sysfs plus logind, `brightnessctl` fallback, DDC/CI for external monitors | Accepted |
| Brightness gamma             | 2.0 for internal only                                                     | Accepted |
| Brightness dim/restore       | Built-in snapshot, dim, and restore commands                              | Accepted |
| App launch                   | Detached double-fork, not Hyprland `exec_cmd`                             | Accepted |
| Tray menu                    | Rust DBusMenu client, no vala-panel-appmenu                               | Accepted |
| Notification settings        | Own GSettings schema, JSON payload store                                  | Accepted |
| Notification placement       | Farthest monitor for the configured corner                                | Accepted |
| Notification display timeout | 5 s, pause on hover, DND suppresses cards                                 | Accepted |
| Bluetooth discovery          | Add a discovery section, scan on dropdown open                            | Accepted |
| VPN toggle                   | Disabled until the VPN service exists                                     | Accepted |
| Multi-monitor bar            | One bar per enabled monitor                                               | Accepted |
| Active windows               | Open on the current monitor                                               | Accepted |
| Monitor configuration        | Generic read, compositor-specific write                                   | Accepted |
| Power profiles               | Add to quick settings now                                                 | Accepted |
| Auth                         | PAM for power menu, lock screen, user switch                              | Accepted |
| Session lock                 | `ext-session-lock-v1`, PAM unlock, logind `Lock` requests                 | Accepted |
| Lock crash recovery          | Runtime marker, re-lock at startup, `misc:allow_session_lock_restore`     | Accepted |
| Polkit prompts               | Shell registers its own authentication agent, PAM through `polkit-1`      | Accepted |
| Polkit caller                | `BeginAuthentication` requires uid 0                                      | Accepted |
| Polkit prompts at once       | One card, a new request cancels the older session                         | Accepted |
| Polkit user name             | Resolved through NSS by uid, never from `$USER`                           | Accepted |
| Custom applets               | TOML entries, script providers, bar applets                               | Accepted |
| Generic workspaces           | Implement both, specific-first try tree                                   | Accepted |

## Definition of done

- Every defect in this file has either a matching acceptance test or a decision entry.
- The Hyprland dispatch formatter has tests for Lua and hyprlang, and for provider and version detection.
- The NetworkManager and BlueZ services include the fixes from the relevant open PRs.
- The tray and notification services do not block the GTK thread.
- The brightness service uses the actor queue and controls internal and DDC displays.
- The monitor service creates one bar per monitor and places notifications on the farthest corner.
- The Rust build has no dependency on Astal, GObject, libnm, libwireplumber, or appmenu-glib-translator.
- The decision log has no unresolved entry for a feature that is about to ship.
