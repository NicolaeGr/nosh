# KDE Connect

Current files: `src/quick_settings/widgets/KDEConnect.vala`, `KDEConnectDropdown.vala`, `src/quick_settings/utils/KDEManager.vala`.

## Current behavior

### Manager

`KdeConnect.Manager` declares D-Bus interfaces:

- `org.kde.kdeconnect.daemon` at `/modules/kdeconnect`.
  - `devices(bool reachable, bool paired) -> string[]`.
  - signals `device_added`, `device_removed`, `device_visibility_changed`.
- `org.kde.kdeconnect.device` at `/modules/kdeconnect/devices/<id>`.
  - properties `id`, `name`, `isReachable`, `isTrusted`, `charge`, `isCharging`.
  - `isPluginEnabled(plugin)`, `setPluginEnabled(plugin, enabled)`.
- `org.kde.kdeconnect.device.clipboard`.
  - `sendClipboard()`.
- `org.kde.kdeconnect.device.share`.
  - `shareUrls(string[] urls)`.

`Manager` calls `init_daemon` in its constructor and starts a 2 second timer that checks the daemon with a proxy. If the daemon appears, it initializes the proxy and emits `daemon_state_changed(true)`. If it disappears, it clears the proxy and emits false.

`set_daemon_state(true)` spawns `kdeconnectd` and retries `init_daemon` after 500 ms. `set_daemon_state(false)` runs `pkill kdeconnectd`, then `killall kdeconnectd` as a fallback.

`get_active_devices` calls `daemon.devices(true, true)` and creates a proxy for each device. `send_files` converts paths to file URIs and calls `shareUrls`.

`toggle_notifications` and `get_notifications_enabled` exist but the UI does not use them.

### Large button

`QuickSettings.Widgets.KDEConnect` extends `LargeButton` with class `QuickSettings-kde-connect`.

- Icon `smartphone-symbolic`.
- Status `KDE Connect`.
- Icon enabled.
- Creates a `Manager`, listens for `daemon_state_changed`.
- When running: status `KDE Connect`, icon enabled, status label without `disabled`.
- When not running: status `KDE Connect (Off)`, icon disabled, status label with `disabled`.

### Dropdown

`KDEConnectDropdown` is a vertical `Gtk.Box` with class `dropdown kde-connect-dropdown`, size request 300 by natural height.

- On `visible`, calls `load_and_display_devices`.
- Header is a `Gtk.CenterBox` with class `dropdown-header`.
- Title label with markup `<b>KDE Connect</b>`.
- Right side: `Gtk.Switch` with class `small-switch`, refresh button `view-refresh-symbolic`, settings button `emblem-system-symbolic`.
- The switch calls `toggle_daemon`.
- The refresh button logs a warning and reloads devices.
- The settings button tries `AppInfo.launch_default_for_uri("kcmshell5")`, which is wrong, then spawns `kdeconnect-app`.
- Two separators with class `dropdown-separator` are appended. This is a duplicate, likely accidental.
- Device box, vertical spacing 4, class `devices-container`, margins 8/8/4/4.
- On daemon state change, updates the switch and clears devices when the daemon stops.
- `load_and_display_devices` reconnects the daemon if needed, gets active devices, clears the box.
- Empty state `No devices found` with class `no-devices`.
- Daemon offline state `Daemon offline`.
- Device card: vertical box, class `device-card`.
  - Top row: phone icon, name, reachability label, trust label, charge label.
  - Buttons: send clipboard `edit-copy-symbolic`, paste clipboard `edit-paste-symbolic`, send file `document-send-symbolic`.
  - All three are TODO and only log a warning.

## Core

Add a `KdeConnectService` to `nosh-core`.

### Connection

Session bus, service `org.kde.kdeconnect`, daemon path `/modules/kdeconnect`.

Do not poll every 2 seconds. Use `NameOwnerChanged` for `org.kde.kdeconnect` and a `zbus` proxy. When the owner appears, create the daemon proxy and read the device list. When it disappears, clear the state.

If the service is not running, expose `KdeConnectEvent::DaemonState(false)`. Do not start it automatically unless the user toggles it.

### Daemon control

```rust
Command::KdeConnectSetDaemon(bool)
```

Preferred start: `systemctl --user start kdeconnectd` if the unit exists. Fallback: spawn `kdeconnectd` with a detached process.

Preferred stop: `systemctl --user stop kdeconnectd`. Fallback: send `SIGTERM` to the process if the service started it. Do not run `pkill` or `killall` blindly; that kills a daemon the user may have started elsewhere and is a footgun.

If the user wants the old behavior, keep `pkill` behind a config flag. Default to the unit or a tracked child.

### Model

```rust
pub struct KdeConnectDevice {
    pub id: String,
    pub name: String,
    pub reachable: bool,
    pub trusted: bool,
    pub charge: i32,
    pub charging: bool,
    pub plugins: BTreeMap<String, bool>,
}
```

Read the device list with `devices(true, true)`. Create a proxy per device at `/modules/kdeconnect/devices/<id>`. Subscribe to `PropertiesChanged` for `isReachable`, `isTrusted`, `charge`, `isCharging`.

The daemon signals `device_added`, `device_removed`, and `device_visibility_changed` update the list. A visibility change means reachability may have changed, so re-read the device.

### Clipboard and files

The current UI has three buttons and no implementation. Decide the scope.

Option A, keep the buttons disabled with a tooltip. This is honest and small.

Option B, implement clipboard send:

- Read the local clipboard selection. `gtk::Clipboard` is a UI API, so read it in the UI and send the text to the core.
- Call `org.kde.kdeconnect.device.clipboard.sendClipboard()`. The daemon reads the clipboard itself; it does not take the text as an argument. This means the shell does not need to send the content, only the signal.
- For paste from the device, KDE Connect normally writes to the clipboard through its plugin. There is no direct `getClipboard` method in the declared interface. Do not invent one.

Option C, implement file send:

- Open a `gtk::FileDialog` in the UI.
- Send the selected paths to the core.
- Convert each path to a `file://` URI.
- Create a proxy for `org.kde.kdeconnect.device.share` at `/modules/kdeconnect/devices/<id>/share`.
- Call `shareUrls(uris)`.

Recommended: implement file send and clipboard send, and remove the paste button unless a supported method is found. Do not leave buttons that only log warnings.

### Notifications plugin

`toggle_notifications` and `get_notifications_enabled` are unused. Do not port them until a UI exposes them.

## UI

### Large button

`KDEConnect` extends `LargeButton` with class `QuickSettings-kde-connect`.

- Icon `smartphone-symbolic`.
- Status `KDE Connect` when the daemon is running, `KDE Connect (Off)` otherwise.
- Icon enabled only when running.
- Click opens the dropdown.

### Dropdown

Header:

- Bold title `KDE Connect`.
- Switch with class `small-switch`, checked when the daemon is running. On change, send `KdeConnectSetDaemon`.
- Refresh button `view-refresh-symbolic`, tooltip `Refresh devices`.
- Settings button `emblem-system-symbolic`, tooltip `Open KDE Connect`.
- Collapse button, matching the other dropdowns. The current KDE Connect dropdown has no collapse button. Add one for consistency.

Remove the duplicate separator.

Content:

- Scrolled window, vertical automatic, min height 240.
- If the daemon is offline, show `Daemon offline` with class `no-devices`.
- If there are no devices, show `No devices found`.
- Device card:
  - Icon `phone-symbolic`, large.
  - Name, ellipsized.
  - Reachable label: `Reachable` or `Offline` with `status-online` or `status-offline`.
  - Trust label: `Trusted` or `Untrusted` with `status-trusted` or `status-untrusted`.
  - Charge label: `{charge}%` when charge >= 0. The current code always prints `0%` if the daemon reports 0. Check whether the device supports charge.
  - Buttons row: send clipboard, paste clipboard, send file. Keep homogeneous.
  - Send clipboard sends `Command::KdeConnectSendClipboard(device_id)`.
  - Paste clipboard is disabled or removed until a supported method exists.
  - Send file opens a file dialog and sends `Command::KdeConnectSendFiles(device_id, paths)`.

Settings button: use `gtk::UriLauncher` with `settings://` or spawn `kdeconnect-app`. Do not pass `kcmshell5` to `launch_default_for_uri`; that is not a URI and the current code will fail.

## CSS

Keep the `.kde-connect-dropdown` rules: `.devices-container`, `.device-card`, `.device-top`, `.device-icon`, `.device-name`, `.status-label`, `.status-online`, `.status-offline`, `.status-trusted`, `.status-untrusted`, `.battery-level`, `.device-buttons`, `.device-action-btn`, `.no-devices`, `.dropdown-separator`, `.dropdown-header`. Remove any rule only for a widget that is truly gone.

## Defects to fix

- Replace the 2 second polling loop with `NameOwnerChanged`.
- Do not use `pkill` or `killall` by default.
- Remove the duplicate separator.
- Use a real settings URI or process, not `AppInfo.launch_default_for_uri("kcmshell5")`.
- Do not log a warning on every refresh.
- Do not ship three buttons that only log warnings.
- Add a collapse button for consistency with the other dropdowns.

## Definition of done

- The quick settings button shows whether the daemon is running.
- The switch starts and stops the daemon through a safe path.
- The dropdown lists reachable, paired devices with name, reachability, trust, and charge.
- Device add, remove, and visibility changes update the list without polling.
- Send file works through a file dialog and `shareUrls`.
- Send clipboard works through the KDE Connect clipboard plugin, or the button is removed.
- No button only logs a warning.
- The daemon stopping clears the list and shows `Daemon offline`.
