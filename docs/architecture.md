# Architecture

Read this before any feature file. It defines where code belongs and how the two crates talk.

## Crate split

### `nosh-core`

No GTK, no `gdk`. It may use `gio` for GSettings and D-Bus helpers. It may depend on:

- `tokio` for the runtime and timers.
- `zbus` for D-Bus clients and servers.
- `serde`, `serde_json`, `toml` for persistence and config.
- `rusqlite` for the app usage database.
- `libpulse-binding` for audio, with `wpctl` as a fallback.
- `wayland-client` and `wayland-protocols` for the idle inhibitor and generic workspace state.
- `gio` for a GSettings schema, with a trait so tests can use an in-memory store.
- `freedesktop-desktop-entry` or a small desktop entry parser for the app index.
- `image`, `sha2`, and `reqwest` or `ureq` for artwork and notification image caches.
- `notify` for file watching where polling would otherwise be needed.
- `tracing` and `thiserror`.
- `bitflags` for per-feature capability sets.
- `async-channel` for the bridge.

It owns:

- Hyprland connection, event stream, monitors, workspaces, clients, dispatch, urgent state.
- Generic `ext-workspace-v1` state when available.
- Monitor layout, current monitor, and farthest-monitor placement.
- Audio devices and volume.
- Brightness devices, including internal, DDC/CI, and command backends.
- UPower battery and power profiles.
- NetworkManager devices, access points, active connections.
- BlueZ adapters and devices.
- MPRIS players.
- StatusNotifier watcher, items, menus.
- Notification daemon and notification store.
- App index, search, usage database, launcher config, custom entries, provider scripts.
- idle inhibitor state.
- PAM authentication.
- CLI command parsing and dispatch.
- Backend resolvers for features with more than one implementation.

It exposes state through a snapshot and a stream of changes. It never references a widget, a window, or a CSS class.

Features with more than one implementation use the try tree in [backends.md](backends.md). Feature code asks for a capability, not a compositor name. The resolver selects the first specific backend that satisfies the capability and falls back to more generic ones.

### `nosh-ui`

Depends on `nosh-core`, `gtk4`, `glib`, `gtk4-layer-shell`, `async-channel`, and `tracing`.

It owns:

- The `gtk::Application` and its command-line handler.
- Every window and widget.
- UI-only state: which window is open, selection index, hover state, dropdown state, animation state.
- CSS, icons, and resources.
- The bridge task that reads core events and mutates widgets on the GTK main thread.

It must not open a D-Bus connection, read `/proc`, read sysfs, spawn a process for system state, or talk to Hyprland directly. The only process spawns in the UI are user-facing launches, and those go through `nosh-core`.

## Runtime model

GTK owns the main thread. The core runs on a dedicated thread with a Tokio runtime. The two threads never share GTK objects.

```text
GTK main thread                         core thread
-----------------                       -----------
GtkApplication                          tokio runtime
  windows/widgets                       services and supervisors
       |                                      |
       | Command  ------------------------>   |
       |                                      |
       | <------------------------- Event      |
       +--- glib::spawn_future_local reads Event channel
```

Use `async_channel` for both directions. It is runtime-agnostic and does not require a Tokio reactor on the GTK side.

### Core API

```rust
pub struct Core {
    commands: async_channel::Sender<Command>,
    events: async_channel::Receiver<Event>,
}

impl Core {
    pub fn start() -> Result<Self, CoreError>;
    pub fn subscribe(&self) -> async_channel::Receiver<Event>;
    pub fn send(&self, command: Command);
}
```

`Event` has two kinds:

- `Snapshot` with the complete current state for every service that has started.
- `Changed` with one typed delta.

The UI applies `Snapshot` on startup and after a core restart. It applies `Changed` for normal updates.

```rust
pub enum Event {
    Snapshot(Snapshot),
    Hyprland(HyprlandEvent),
    Audio(AudioEvent),
    Brightness(BrightnessEvent),
    Battery(BatteryEvent),
    Network(NetworkEvent),
    Bluetooth(BluetoothEvent),
    Media(MediaEvent),
    Tray(TrayEvent),
    Notifications(NotificationEvent),
    KdeConnect(KdeConnectEvent),
    IdleInhibit(IdleInhibitEvent),
    Apps(AppsEvent),
    ServiceAvailability(Service, bool),
}
```

`Command` carries user intent. It must be cheap to send and must not block.

```rust
pub enum Command {
    AudioSetVolume(f64),
    AudioStepVolume(f64),
    AudioSetMuted(bool),
    BrightnessSet(f64),
    BrightnessStep(f64),
    BrightnessSnapshot,
    BrightnessDim { value: f64 },
    BrightnessRestore,
    BrightnessRescan,
    WorkspaceActivate(String),
    WorkspaceMoveToMonitor { workspace: String, monitor: String },
    HyprlandFocusClient(String),
    WifiSetEnabled(bool),
    WifiScan,
    WifiActivate { ssid: String, password: Option<String>, bssid: Option<String> },
    WifiDeactivate,
    BluetoothSetPowered(bool),
    BluetoothConnect(String),
    BluetoothDisconnect(String),
    NotificationDismiss(u32),
    NotificationInvoke { id: u32, action: String },
    NotificationSetDnd(bool),
    AppReload,
    AppLaunch(AppEntry),
    AppRecordLaunch(String),
    IdleInhibitSet(bool),
    PowerProfileSet(PowerProfile),
    PowerProfileCycle,
    MonitorApply(MonitorLayout),
    MonitorCycle { target: String, mode: Option<LayoutPreset> },
    MonitorMode { target: String, mode: LayoutPreset },
    MonitorSetPrimary(String),
    MonitorSetEnabled { connector: String, enabled: bool },
    AuthAuthenticate(AuthRequest),
    AuthCancel,
    KdeConnectSetDaemon(bool),
    KdeConnectSendClipboard(String),
    KdeConnectSendFiles { device: String, paths: Vec<PathBuf> },
    TrayActivate { item_id: String, x: i32, y: i32 },
    TraySecondaryActivate { item_id: String, x: i32, y: i32 },
    TrayMenuAboutToShow(String),
    TrayMenuEvent { item_id: String, menu_id: i32, event: String },
}
```

Feature files list the exact commands they need. Do not add a command for state the UI can own. Window visibility, selection index, and dropdown state stay in the UI.

### Startup sequence

1. `main` initializes logging and starts `Core::start()`.
2. `Core::start()` starts every service concurrently. A service that cannot connect reports `ServiceAvailability(service, false)` and retries in the background. It does not abort startup.
3. The UI creates the application and windows, but keeps nonessential widgets hidden until the first `Snapshot`.
4. The UI subscribes to events before creating windows, so it cannot miss the first snapshot.
5. The UI runs `glib::spawn_future_local` to consume events and call a pure `apply_event(&mut UiState, Event)` followed by targeted widget updates.

Never block the GTK thread waiting for a core reply. If a request needs a reply, use a `oneshot` channel inside the command and send the reply as an `Event`.

## State ownership

| State                                              | Owner |
| -------------------------------------------------- | ----- |
| Hyprland monitors, workspaces, clients, urgent set | core  |
| Generic workspaces from `ext-workspace-v1`         | core  |
| Monitor layout, current monitor, placement rules   | core  |
| Audio devices, default sink/source, volume, mute   | core  |
| Brightness devices, DDC displays, snapshot state   | core  |
| Battery and power profiles                         | core  |
| Network devices, access points, connections        | core  |
| Bluetooth adapters and devices                     | core  |
| MPRIS players and metadata                         | core  |
| Tray items and menus                               | core  |
| Notifications and daemon settings                  | core  |
| App index, usage database, launcher config         | core  |
| Idle inhibitor availability and active state       | core  |
| PAM authentication state                           | core  |
| Which quick settings dropdown is open              | UI    |
| App launcher query and selected index              | UI    |
| Quick settings and app launcher visibility         | UI    |
| Notification card hover and display timers         | UI    |
| Change indicator visible state and hide timer      | UI    |

The idle inhibitor active state is system state, so the core owns it. The bar button mirrors it and sends `IdleInhibitSet`.

The old `State.AppState` singleton disappears. Its three fields split:

- `quick_settings_open` stays in the UI.
- `app_launcher_open` stays in the UI.
- `idle_inhibitor_active` moves to the core and is mirrored by the UI.

Do not create a new global singleton. Pass the UI state through the application struct and the core through a handle.

## Backend resolution

Monitors, workspaces, audio, brightness, notifications, and tray have more than one possible implementation. Use an ordered try tree, specific first, generic last. Each backend has a probe, a capability set, and a runtime failure path. The resolver caches the selection, emits the selected backend in the state snapshot, and falls back on failure.

Do not write `if compositor == "hyprland"` in a feature. That check belongs in the Hyprland backend's probe. Do not lose the whole feature when a write capability is missing. A bar can still read monitors and place notifications when the compositor has no write API.

See [backends.md](backends.md) for the pattern and the per-feature trees.

## Error handling and reconnection

Every service implements the same supervisor shape:

```text
connect -> subscribe -> emit availability true -> run
   ^                                                |
   +---------------- backoff <---- error or EOF <---+
```

Rules:

- Log with `tracing`, including the service name and the D-Bus name or socket path.
- Exponential backoff from 250 ms to 30 s, with a small jitter.
- Emit `ServiceAvailability(service, false)` when disconnected. The UI disables or hides the dependent widget.
- Emit a fresh snapshot after reconnect. The UI must replace, not merge, the affected service state.
- Never panic in a long-lived task. A panic in a service is caught by the task supervisor and restarted.
- Treat a missing D-Bus service or compositor protocol as unavailable, not fatal. The bar must still start without NetworkManager, BlueZ, UPower, or a tray.
- On a backend failure, degrade to the next candidate in the try tree before declaring the feature unavailable.

## Window rules

All windows use `gtk4-layer-shell`. The crate calls `gtk4_layer_shell::init_for_window(&window)` before the window is realized, then sets anchors, layer, exclusivity, keyboard mode, namespace, and margins.

Use a `gtk::Window` with decorations disabled and resizing disabled. Add it to the application with `app.add_window(&window)`.

| Window           | Anchor              | Layer   | Exclusivity | Keymode   | Namespace                  | Monitor | Margins         |
| ---------------- | ------------------- | ------- | ----------- | --------- | -------------------------- | ------- | --------------- |
| Bar, one per monitor | top, left, right | top | exclusive | none | `nosh-topbar` | each enabled monitor | none |
| Quick settings   | all four            | overlay | ignore      | exclusive | `nosh-quick-settings`      | current | none |
| Notifications    | configured corner   | overlay | ignore      | none      | `nosh-notifications-outer` | farthest for corner | 8 / 28 |
| App launcher     | all four            | overlay | ignore      | exclusive | `nosh-app-launcher`        | current | none |
| Change indicator | bottom, left, right | top     | ignore      | none      | `nosh-change-indicator`    | current | none |

The bar is the only exclusive window. It must request an exclusive zone so Hyprland reserves space for it.

Current-monitor windows: quick settings, the app launcher, and the change indicator. Set the layer-shell monitor to the compositor's focused monitor before presenting. If the window is already mapped, hide it, set the monitor, then present. The notification window uses the farthest-monitor rule from `features/monitors.md`, not the current monitor.

Layer shell windows need a nonzero size request before mapping. Set `width_request = 1` and `height_request = 1` at construction like `Astal.Window` does, then let the content expand.

For popovers from tray icons or dropdowns, use normal GTK popovers. Do not create a second layer surface unless the popover must escape a clipping layer window.

## Application lifecycle and IPC

Use `gtk::Application` with `ApplicationFlags::HANDLES_COMMAND_LINE` and the existing application ids. The stable id is `com.nicolaegr.nosh`; the dev id is `com.nicolaegr.nosh.dev`.

The current shell is single-instance through `GApplication`. A second `nosh volume-up` reaches the first process through `command_line`. Keep that behavior. Do not add a separate D-Bus service for IPC unless a future feature needs replies.

`command_line` maps the first argument:

| Argument          | Action                                    |
| ----------------- | ----------------------------------------- |
| `volume-up`       | send `AudioStepVolume(+0.05)`             |
| `volume-down`     | send `AudioStepVolume(-0.05)`             |
| `brightness-up`   | send `BrightnessStep(+5.0)`               |
| `brightness-down` | send `BrightnessStep(-5.0)`               |
| `brightness-dim`  | send `BrightnessDim { value: 10.0 }`      |
| `brightness-restore` | send `BrightnessRestore`               |
| `app-launcher`    | toggle app launcher visibility in the UI  |
| `monitor-cycle`   | send `MonitorCycle` for the next monitor  |
| `power-profile`   | send `PowerProfileSet` for the next profile |
| anything else     | print the current error text and return 0 |

The current error text and exit behavior are in `features/command-line.md`.

In a dev build, the current Vala code stops `nosh.service`, starts the dev instance, and restarts the stable service when the dev bar closes. Keep this behind a `dev` Cargo feature. A release build must never call `systemctl`.

## Config and state paths

| Path                                     | Purpose                                            |
| ---------------------------------------- | -------------------------------------------------- |
| `~/.config/nosh/app_launcher.toml`       | custom launcher entries and script providers       |
| `~/.config/nosh/config.toml`             | new: bar, time format, applet, and appearance settings |
| `~/.config/nosh/monitors.toml`           | new: monitor layout presets and optional startup apply |
| `~/.local/share/nosh/app_launcher.db`    | SQLite usage database                              |
| `~/.cache/nosh/covers/`                  | MPRIS cover art cache                              |
| `~/.cache/nosh/notifications/`           | notification image cache                           |
| `~/.cache/nosh/brightness/snapshot.json` | brightness save/dim/restore state                  |
| `~/.local/state/nosh/notifications.json` | persisted notification store                       |
| `$XDG_RUNTIME_DIR/nosh/`                 | future socket or lock files                        |

Use the `directories` crate or `glib::user_config_dir()` in the UI. The core should use `std::env` plus `xdg` path helpers so it stays GTK-free. Resolve paths once at startup and pass them into services.

Settings that need live change notification, a `gsettings` CLI, and typed keys live in our own GSettings schema, for example `com.nicolaegr.nosh`. Notification payloads stay in `notifications.json`, not GSettings. App launcher entries stay in TOML. Monitor layout stays in `monitors.toml` because it is compositor-specific and may be applied at startup.

Settings inventory:

| Group | Keys | Store |
| --- | --- | --- |
| Notifications | `ignore-timeout`, `dont-disturb`, `default-timeout`, `max-visible`, `display-timeout`, `position`, `monitor-policy`, `server-capabilities` | GSettings |
| Bar | `time-format`, `show-secondary` (dev), custom applet order | GSettings and `config.toml` |
| Brightness | `step`, `dim-value`, `backend`, `ddc-backend` | `config.toml` |
| Auth | `service`, `lockout-attempts`, `lockout-seconds` | `config.toml` |
| Monitor layout | `apply-on-start`, presets, per-connector layout | `monitors.toml` |
| App launcher | `[[entry]]`, `[[provider]]`, `[[applet]]` | `app_launcher.toml` |
| Notification payloads | persisted notifications | `notifications.json` |
| Brightness snapshot | saved values before dim | `brightness/snapshot.json` |

The current Astal notification settings live in `io.astal.notifd`. On first run, if that schema exists, import `ignore-timeout`, `dont-disturb`, and `default-timeout` once. Do not depend on Astal or that schema at runtime.

## Styling and resources

Keep the SCSS sources. They are part of the visual contract and the utility classes are used by the widgets. Compile `src/style.scss` in `build.rs` with `grass`. If `grass` cannot reproduce `sass` output, use the `sass` CLI in `build.rs` and fail the build with a clear message when it is missing.

Load the result with `gtk::CssProvider::load_from_string` and add it at `STYLE_PROVIDER_PRIORITY_USER`. Keep the resource prefix `com.nicolaegr.nosh` if you use GResource.

GTK CSS functions such as `alpha()`, `mix()`, `lighter()`, and `darker()` are passed through as strings by the existing SCSS helpers. Do not let a Sass implementation evaluate them. The generated CSS must contain those function calls for GTK to resolve.

Utility classes such as `min-h-6`, `px-1`, `gap-2`, `rounded-2xl`, `text-sm`, and `opacity-60` come from `src/style/common/`. Keep them. Do not replace them with a Tailwind build.

Icons come from the icon theme. The system stats widget currently uses Nerd Font private-use glyphs. Decide whether to keep them or switch to symbolic icons. If you keep them, document the font requirement and provide a fallback label.

## Reference material

Protocol and build shapes live in [reference/README.md](reference/README.md). The D-Bus, Hyprland IPC, Wayland, GTK, and build files there are original pseudocode summaries, not copied implementation source. Use them with the feature specs. Verify the installed service or compositor version before relying on a signature.

## Build features

| Feature  | Effect                                                                                                                  |
| -------- | ----------------------------------------------------------------------------------------------------------------------- |
| `dev`    | use `com.nicolaegr.nosh.dev`, add `secondary-instance` to the bar, stop and restart the stable service around a dev run |
| `stable` | use `com.nicolaegr.nosh` and no dev behavior                                                                            |

Only one of `dev` and `stable` may be enabled. Default to `dev` for local builds.

The release binary installs as `nosh`. The Vala project uses Meson, but the Rust workspace can use `cargo` plus a small `install` step that copies the binary, the CSS, and a sample config.

## Language mapping

| Vala / GObject                    | Rust                                                              |
| --------------------------------- | ----------------------------------------------------------------- |
| `signal void changed()`           | `Event` variant sent through a channel                            |
| `bind_property`                   | snapshot update in the UI; `glib::Binding` for local widget state |
| `notify["prop"].connect`          | `zbus::Proxy::receive_properties_changed` or a typed watch stream |
| Vala `async`                      | `tokio::spawn` and `async fn`                                     |
| `GLib.Timeout`                    | `tokio::time::sleep` in core, `glib::timeout_add_local` in UI     |
| `Gee.ArrayList`                   | `Vec`                                                             |
| `HashTable`                       | `HashMap` or `BTreeMap`                                           |
| GObject refcount                  | `Arc`, with `Weak` for parent or observer links                   |
| `GtkTemplate` / Blueprint         | build widgets in Rust, or load a GtkBuilder XML file              |
| `Astal.Window`                    | `gtk::Window` plus `gtk4-layer-shell`                             |
| Astal property binding to widgets | explicit update function per widget                               |

Rust makes the interactive parts easier: channels, typed state, `Result`, and tests. GTK still wants the main thread, so keep all widget mutation in the GTK thread and make the bridge one-directional per event.

## Testing

Core tests:

- Hyprland event parser and dispatch formatter, both config providers, plus version and provider detection.
- Hyprland reconnect after socket EOF and missing socket.
- Workspace filtering, special workspace exclusion, and generic group to monitor mapping.
- Monitor placement score for all four corners and tie-breaking.
- Fuzzy scoring tables from `AppProvider`.
- TOML config parsing, including the `# EXAMPLE_CONFIG` sentinel and script provider entries.
- SQLite schema and upsert.
- Brightness gamma mapping, clamping, DDC value mapping, snapshot, dim, and restore.
- Audio volume clamping at 0 and 1.5, and the bar cap at 1.0.
- Notification serialization, replacement, timeout, action invocation, and placement.
- Network access point grouping and security selection.
- BlueZ adapter state transitions and discovery.
- Power profile string mapping.
- Tray menu model flattening and DBusMenu item types.
- PAM conversation state without logging the password.
- Detached spawn helper: child survives parent exit and does not become a zombie.

Use `zbus::blocking::Connection::session` against a private `dbus-daemon` for D-Bus integration tests, or a fake trait implementation. Do not require a live desktop for `cargo test`.

UI tests:

- A smoke test that starts the app under `weston` or a nested Hyprland and checks that all five windows map with the right namespace.
- Manual checks for exclusive-zone height, keyboard grab, popover placement, and dropdown animation.

## Definition of done

- `cargo test -p nosh-core` passes without a display or a running desktop service.
- `cargo build -p nosh-ui` has no GTK calls in the core dependency graph.
- Every window maps with the namespace in the table above.
- A missing D-Bus service leaves the bar running and the dependent widget hidden or disabled.
- The core survives a Hyprland restart, a NetworkManager restart, and a BlueZ restart without restarting the process.
- The CLI command names and application ids match the table in `README.md`.
