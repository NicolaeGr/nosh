# Bluetooth

Current files: `src/quick_settings/widgets/Bluetooth.vala`, `BluetoothDropdown.vala`, `src/quick_settings/widgets/Connectivity.vala`.

There is no Bluetooth widget in the bar. Bluetooth appears only in quick settings.

## Current behavior

### Availability

`Connectivity.is_bluetooth_available()` makes a synchronous `GetNameOwner` call on the system bus for `org.bluez` with a 500 ms timeout. If it fails, the Bluetooth button and dropdown are not created.

### Large button

`QuickSettings.Widgets.Bluetooth` extends `LargeButton` with class `QuickSettings-bluetooth`.

- Gets `AstalBluetooth.get_default()`; hides itself if null.
- `update_status`:
  - Powered: status `Bluetooth`, icon `bluetooth-active-symbolic`, icon enabled.
  - Not powered: status `Bluetooth Off`, icon `bluetooth-disabled-symbolic`, icon disabled.
- Updates on `notify["is-powered"]`.

### Dropdown

`BluetoothDropdown` is a vertical `Gtk.Box` with class `dropdown`.

Header, spacing 8, padding 8, class `header`:

- Label `Bluetooth`.
- `Gtk.Switch` with class `small-switch`, bound bidirectionally to `is-powered`.
- Refresh button `view-refresh-symbolic`, 24x24, calls `update_devices()`.
- Settings button `preferences-system-symbolic`, 24x24, opens `settings://bluetooth`, then falls back to `gnome-control-center bluetooth`.
- Collapse button `go-up-symbolic`, 24x24, hides the dropdown.

Then a separator with class `divider`, then a scrolled window with vertical automatic and minimum height 240.

`update_devices` removes every child and rebuilds.

- If not powered, shows `Bluetooth is off` with class `empty-state`.
- Else builds a list of paired devices and sorts connected devices first.
- Each device item is a button with a horizontal box, class `device-item`.
- Icon: `device.icon` if non-empty, else `bluetooth-symbolic`.
- Text: device name, class `network-name dim-label`, and status `Connected` with class `connected`, or `Available`.
- Click connects or disconnects the device.
- If no paired devices, shows `No paired devices` with class `empty-state`.

The refresh button only rebuilds the list. It does not start discovery.

Decision: the rewrite adds a discovery section. Opening the Bluetooth dropdown from quick settings starts a scan automatically. The refresh button restarts the scan.

The device icon is the raw BlueZ icon, for example `audio-headset`, without the `-symbolic` suffix.

## Core

Add a `BluetoothService` to `nosh-core`. Use `zbus` or `bluez-async` against the BlueZ object manager.

### D-Bus

System bus, service `org.bluez`, root `/`.

Do not probe availability with a blocking `GetNameOwner` call. Use the object manager and a name watcher. The service is available when the bus name has an owner and `GetManagedObjects` succeeds.

Use `org.freedesktop.DBus.ObjectManager`:

- `GetManagedObjects`.
- `InterfacesAdded`, `InterfacesRemoved`.

Interfaces:

- `org.bluez.Adapter1`.
- `org.bluez.Device1`.
- `org.bluez.Battery1`.

Adapter properties:

```text
Powered, Discoverable, Pairable, Discovering, Alias, Name, Address,
DiscoverableTimeout, PairableTimeout, UUIDs, Modalias, Class
```

Device properties:

```text
Connected, Paired, Trusted, Blocked, Alias, Name, Address, Icon,
RSSI, Adapter, UUIDs, Appearance, Class, LegacyPairing, Modalias
```

Battery properties:

```text
Percentage, Source
```

### Adapter state

Do not expose only a bool. Use the state machine from open PR #479:

```rust
pub enum AdapterState {
    Absent,
    Off,
    On,
    TurningOn,
    TurningOff,
}
```

On `Powered` change, settle to `On` or `Off`. On a user toggle, set `TurningOn` or `TurningOff`, start a 30 second timeout, then settle from the actual `Powered` value. Emit the state change immediately so the switch can show an in-progress state.

`is_powered` is true when any adapter is powered. `is_connected` is true when any device is connected.

### Device model

```rust
pub struct BluetoothDeviceModel {
    pub object_path: String,
    pub address: String,
    pub name: String,
    pub alias: String,
    pub icon_name: String,
    pub connected: bool,
    pub paired: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub battery_percentage: Option<u8>,
    pub rssi: Option<i16>,
    pub connecting: bool,
}
```

`icon_name` is `icon + "-symbolic"` when `icon` is non-empty, else `bluetooth-symbolic`. Do not duplicate this in the UI.

`battery_percentage` comes from `org.bluez.Battery1`. If the battery interface is absent, use `None`, not `-1` or `0`.

### Interface ordering

Handle `Adapter1` and `Device1` before `Battery1`. The current Astal code can process a battery before its device and drop it. When a battery interface is removed, disconnect its handler and clear the value.

### Commands

```rust
Command::BluetoothSetPowered(bool)
Command::BluetoothConnect(String)      // object path
Command::BluetoothDisconnect(String)
Command::BluetoothStartDiscovery
Command::BluetoothStopDiscovery
Command::BluetoothPair(String)
Command::BluetoothRemove(String)
```

All D-Bus calls are async. Use a command result event for errors. Do not block.

### Discovery

Implement discovery now. The user wants a discovery section and wants the scan to start when the nested menu opens.

- Call `Adapter1.StartDiscovery` when the Bluetooth dropdown opens from quick settings.
- Also call it when the refresh button is pressed, if a scan is not already running.
- Watch `InterfacesAdded` and `InterfacesRemoved` for new and disappearing devices.
- Stop after a timeout, default 30 seconds. Stop early when the dropdown closes if the scan is still running, unless the user pinned it.
- Show a separate `Available devices` section below the paired devices.
- Sort available devices by signal strength when `RSSI` is present, then by alias.
- Do not show a device that is already in the paired list.
- Do not show a device with an empty alias and an empty name. Use the address as a last resort.

Pairing:

- Clicking an available unpaired device starts pairing with `Device1.Pair`.
- BlueZ may call `org.bluez.Agent1` methods. Implement a minimal agent that handles `RequestConfirmation`, `RequestAuthorization`, `RequestPinCode`, `RequestPasskey`, `DisplayPinCode`, `DisplayPasskey`, `AuthorizeService`, and `Cancel`.
- Show a modal confirmation dialog for `RequestConfirmation` and `RequestAuthorization`. Accept a PIN or passkey for the `Request*` methods. Reject unknown agents by default.
- Register the agent with `AgentManager1.RegisterAgent` and `RequestDefaultAgent`.
- On success, mark the device paired and move it to the paired section.
- On failure, show one error and keep the device in the available section.
- Do not auto-trust a device. Trust is a separate action.

If the agent implementation is deferred, ship discovery without pairing first. Do not add click-to-pair buttons that cannot complete.

### Logging

BlueZ errors such as `AlreadyConnected`, `NotReady`, and `InProgress` are normal races. Log them at debug level, not as critical. The current Vala code logs a warning for every failed connect.

## UI

### Large button

`Bluetooth` extends `LargeButton` with class `QuickSettings-bluetooth`.

- Hide when `AdapterState::Absent` and no adapter exists.
- Icon:
  - `TurningOn` or `TurningOff`: `bluetooth-acquiring-symbolic`.
  - `Off` or `Absent`: `bluetooth-disabled-symbolic`.
  - `On` and connected: `bluetooth-active-symbolic`.
  - `On` and not connected: `bluetooth-disconnected-symbolic`.
- Status: `Bluetooth`, `Bluetooth Off`, `Turning On`, or `Turning Off` depending on the state. Keep it short for the 10-character ticker.
- Icon enabled for `On`.
- Click opens the dropdown.

### Dropdown

Header:

- Label `Bluetooth`.
- `gtk::Switch` with class `small-switch`. Bind it to `AdapterState::On` for the checked state. On change, send `BluetoothSetPowered`.
- While `TurningOn` or `TurningOff`, disable the switch and show the acquiring icon.
- Refresh button. Send `BluetoothStartDiscovery` and restart the timeout.
- Settings button. Prefer `gtk::UriLauncher` or `gio::AppInfo::launch_default_for_uri("settings://bluetooth")`. The fallback `gnome-control-center bluetooth` is acceptable but should be behind a `which` check or a spawn error log.
- Collapse button.

Content:

- Scrolled window, vertical automatic, min height 240.
- If `AdapterState::Off`, show `Bluetooth is off` with class `empty-state`.
- On open, send `BluetoothStartDiscovery` and show a scanning indicator while `discovering` is true.
- Paired section: list paired devices, connected first, then by alias.
- Row: icon, name, status `Connected` or `Available`.
- Click sends connect or disconnect for the object path.
- If no paired devices, show `No paired devices` in the paired section.
- Available section: show discovered, unpaired devices while a scan is running or within the scan result window.
- Row: icon, name, status `Available` or the signal strength when `RSSI` is present. Click starts pairing.
- If no available devices, show `No devices found`.
- Optionally show battery percentage when `battery_percentage` is present.

Use a keyed list and update rows in place. Do not rebuild the entire list on every property change.

### CSS

Keep the current `.QuickSettings-bluetooth`, `.device-item`, `.network-name`, `.network-status`, `.connected`, and `.empty-state` rules.

## Defects to fix

- Blocking `StartDiscovery`, `StopDiscovery`, `Pair`, `ConnectProfile`, and `DisconnectProfile` calls become async.
- The adapter has a state machine, not a bool.
- Battery interfaces are attached after devices and detached on removal.
- The icon name has the `-symbolic` suffix and a fallback.
- The refresh button starts a real scan.
- Opening the dropdown starts a scan and shows discovered devices.
- The switch reflects an in-progress state and cannot be spammed.
- BlueZ races are logged once at debug level.

## Definition of done

- The quick settings button shows powered, off, connecting, and connected states.
- The switch powers the adapter on and off, and reflects the transition.
- The dropdown lists paired devices, connected first.
- Clicking a paired device connects or disconnects it.
- A paired device with a battery shows its percentage when the battery interface exists.
- Opening the dropdown starts a scan and shows an `Available devices` section.
- The refresh button restarts the scan.
- An available unpaired device can be paired through a confirmation dialog, or the discovery section ships without pairing and says so.
- Removing a device or its battery removes the row and the value.
- No blocking D-Bus call runs on the GTK thread.
- The service starts and retries when `org.bluez` is not present.
