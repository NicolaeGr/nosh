# Network and Wi-Fi

Current files: `src/bar/widgets/Wifi.vala`, `src/quick_settings/widgets/WiFi.vala`, `WiFiDropdown.vala`, `Connectivity.vala`.

## Current behavior

### Bar widget

`Wifi` is a `Gtk.Button` with class `square-icon`, containing an image.

- Gets `AstalNetwork.get_default()` and `network.wifi`.
- If `wifi` is null, hides the button.
- Binds `ssid` to `tooltip-text`.
- Binds `icon-name` to the image.

There is no click action and no menu.

### Quick settings large button

`QuickSettings.Widgets.WiFi` extends `LargeButton` with class `QuickSettings-wifi`.

- Hides itself if `network.wifi` is null.
- `update_status` sets `is_connected = wifi.active_access_point != null`.
- Icon is `wifi.icon_name` or `network-wireless-offline-symbolic`.
- Status is `wifi.ssid` when connected, else `WiFi Off`.
- `set_icon_enabled` follows the connection state.
- Updates on `icon-name`, `active-access-point`, and `ssid`.

### Dropdown

`WiFiDropdown` is a vertical `Gtk.Box` with class `dropdown`, expanding horizontally.

Header, spacing 8, padding 8, class `header`:

- Label `WiFi`.
- `Gtk.Switch` with class `small-switch`, bound bidirectionally to `wifi.enabled`.
- Refresh button `view-refresh-symbolic`, 24x24, calls `wifi.scan()` and rebuilds after 500 ms.
- Collapse button `go-up-symbolic`, 24x24, hides the dropdown.

Then a separator with class `divider`, then a scrolled window with vertical policy automatic, no horizontal policy, minimum content height 240.

The content box is vertical. `update_networks` removes every child and rebuilds it.

- If there are no access points, shows `No WiFi networks available` with class `empty-state`.
- Otherwise copies the access points into a list and sorts by `strength` descending.
- Each item is a button containing a horizontal box with class `network-item`.

Item content:

- Signal icon from strength: `>= 80` excellent, `>= 60` good, `>= 40` ok, `>= 20` weak, else none.
- Vertical info box with the SSID label, class `network-name dim-label`, and a status label, class `network-status`.
- Status is `Connected` with class `connected` if `wifi.active_access_point == ap`, else `Signal: {strength}%`.
- Click deactivates the active connection or calls `ap.activate.begin(null)`.

The SSID is used directly as the label. There is no password prompt and no security indicator.

### Availability

`Connectivity` checks whether Wi-Fi hardware exists with `network != null && network.wifi != null`. It checks Bluetooth with a synchronous `GetNameOwner` call on the system bus with a 500 ms timeout. KDE Connect is always added.

## Core

Add a `NetworkService` to `nosh-core`. Use NetworkManager over D-Bus with `zbus`. Do not link libnm.

### D-Bus objects

System bus, service `org.freedesktop.NetworkManager`, path `/org/freedesktop/NetworkManager`.

Watch:

- `DeviceAdded(o)`, `DeviceRemoved(o)`.
- `PropertiesChanged` for `WirelessEnabled`, `State`, `Connectivity`, `PrimaryConnection`, `ActivatingConnection`.

For each device with `DeviceType == 2` (Wi-Fi), use `org.freedesktop.NetworkManager.Device.Wireless`:

- `AccessPoints` (`ao`).
- `ActiveAccessPoint` (`o`).
- `RequestScan(a{sv})`.
- `GetAccessPoints`.
- `GetAllAccessPoints`.

For each access point, use `org.freedesktop.NetworkManager.AccessPoint`:

- `Ssid` (`ay`).
- `Bssid` (`s`, may be null).
- `Strength` (`y`).
- `Frequency` (`u`).
- `Bandwidth` (`u`).
- `MaxBitrate` (`u`).
- `Mode` (`u`).
- `Flags`, `WpaFlags`, `RsnFlags` (`u`).
- `LastSeen` (`i`).

`Ssid` is a byte array, not a string. Convert it with the NetworkManager SSID-to-UTF-8 rule. NetworkManager's C helper `nm_utils_ssid_to_utf8` is the reference. Implement the same escaping in Rust. Do not lossy-decode arbitrary bytes without documenting the replacement character.

### Model

```rust
pub struct WifiModel {
    pub available: bool,
    pub enabled: bool,
    pub scanning: bool,
    pub state: DeviceState,
    pub connectivity: Connectivity,
    pub active_ssid: Option<String>,
    pub active_bssid: Option<String>,
    pub icon_name: String,
    pub access_points: Vec<AccessPointModel>,
}

pub struct AccessPointModel {
    pub id: String,              // stable internal id
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub mode: WifiMode,
    pub security: SecurityType,
    pub strength: u8,
    pub frequency: u32,
    pub bandwidth: u32,
    pub max_bitrate: u32,
    pub requires_password: bool,
    pub access_point_count: usize,
    pub active: bool,
}
```

### Grouping

A dual-band router advertises one access point per radio. Group them into one row per network. The key is `(ssid, mode, security)`. Keep every member in the group.

- The displayed `strength` is the maximum strength of the group.
- `access_point_count` is the number of members.
- `bssid`, `frequency`, and `bandwidth` come from the strongest member.
- An active member makes the group active.
- When a member is removed, recompute the strongest and the active state. If the group is empty, keep the last known SSID for a short grace period so a row does not vanish while NetworkManager rebuilds.

This is the design in open PR #477. Implement it now.

### Security

Detect the security type with the NetworkManager helper order from PR #477:

```text
WPA3_SUITE_B_192
SAE
OWE
WPA2_ENTERPRISE
WPA2_PSK
WPA_ENTERPRISE
WPA_PSK
DYNAMIC_WEP
LEAP
STATIC_WEP
NONE
```

Use `nm_utils_security_valid` semantics for the device capabilities and AP flags. If the result is `INVALID`, the device cannot connect to this network. Hide it or disable it.

`requires_password` is true for static WEP, WPA-PSK, WPA2-PSK, and SAE. It is false for open and OWE. Enterprise networks need an 802.1X profile; a password alone cannot describe them.

### Connect

```rust
Command::WifiActivate { ssid: String, password: Option<String>, bssid: Option<String> }
```

The core:

- Finds an existing connection profile that matches the SSID and security.
- If found and a password is supplied, updates the PSK or WEP key and commits.
- If not found and the network is open, creates a wireless setting and activates.
- If not found and the network needs a password, creates the correct security setting: `none` with a WEP key, `wpa-psk`, `sae`, or `owe`.
- If the network is enterprise, returns an error telling the user to create a profile first.
- Does not pin the BSSID when the network is grouped. Pinning prevents roaming between the group's access points.

This is the behavior of PR #477. Keep it.

```rust
Command::WifiDeactivate
Command::WifiSetEnabled(bool)
Command::WifiScan
```

`WifiScan` calls `RequestScan` and sets `scanning` until `LastScan` changes or a timeout expires.

### Hot-plug

Watch `DeviceAdded` and `DeviceRemoved`. Keep a stable `WifiModel` wrapper even when the device disappears:

- `available = false`.
- `enabled` keeps the last known value or false.
- `access_points` clears.
- `icon_name` becomes offline or disabled.
- The bar widget hides when `available` is false and reappears when a device is added.

This is the behavior of PR #449.

### Icon

Use the same icon rules as AstalNetwork:

```text
disabled -> network-wireless-disabled-symbolic
acquiring -> network-wireless-acquiring-symbolic
connected + hotspot -> network-wireless-hotspot-symbolic
connected + no full connectivity -> network-wireless-no-route-symbolic
connected + no active AP -> network-wireless-connected-symbolic
connected + strength >= 80 -> network-wireless-signal-excellent-symbolic
>= 60 -> good, >= 40 -> ok, >= 20 -> weak, else none
offline -> network-wireless-offline-symbolic
```

Compute it in the core.

### VPN

Do not add VPN to this service in v1. The quick settings VPN toggle stays disabled until a `VpnService` exists. Open PR #478 describes the design: a profile list keyed by UUID, `is_active`, `icon_name`, async activate and deactivate, and an `activation_failed` reason. Cover both `vpn` and `wireguard` connection types.

## UI

### Bar

`Wifi` is a `gtk::Button` with class `square-icon`, a `gtk::Image` child, and a tooltip.

- Hide when `!available`.
- Icon from `WifiModel.icon_name`.
- Tooltip from `active_ssid`, or empty when disconnected.
- No click action in v1. If a click opens quick settings later, send a UI action, not a core command.

### Quick settings large button

`WiFi` extends `LargeButton` with class `QuickSettings-wifi`.

- Hide when `!available`.
- Icon `icon_name` or `network-wireless-offline-symbolic`.
- Status `active_ssid` when active, else `WiFi Off`.
- `set_icon_enabled(active)`.
- Click opens or closes the Wi-Fi dropdown.

### Dropdown

Header:

- Label `WiFi`.
- `gtk::Switch` with class `small-switch`, bound to `enabled`. On change, send `WifiSetEnabled`.
- Refresh button `view-refresh-symbolic`, 24x24, sends `WifiScan`.
- Collapse button `go-up-symbolic`, 24x24, hides the dropdown.

The dropdown should use a `gtk::Revealer` with `SLIDE_DOWN`, 300 ms, instead of the current CSS `max-height` hack. The current `Connectivity.toggle_dropdown` loads CSS into a shared provider, waits 310 ms, and changes `visible`. Replace it with:

- A `gtk::Revealer` whose child is the dropdown content.
- `set_reveal_child(true)` on open, `false` on close.
- `set_transition_duration(300)` and `set_transition_type(RevealerTransitionType::SlideDown)`.
- Only one dropdown open at a time.
- Close all dropdowns when quick settings closes.

Content:

- Scrolled window, vertical automatic, min height 240.
- One row per access point group, sorted by strength descending, then SSID.
- Row icon from strength.
- SSID label. If `ssid` is null, show `Hidden network` and do not print an empty label.
- Status `Connected` with class `connected` when active, else `Signal: {strength}%`.
- If `requires_password` and not connected, show a lock indicator. The current UI has none.
- Click an active network: send `WifiDeactivate`.
- Click a network with a saved profile: send `WifiActivate` with no password.
- Click a secured network with no saved profile: open a password dialog, then send `WifiActivate`.
- Click an open network: send `WifiActivate`.
- Click an enterprise network with no profile: show an error message, do not attempt activation.
- Empty state `No WiFi networks available` with class `empty-state`.

Add a `gtk::PasswordEntry` inside the dropdown, inserted under the selected
network row, with a Cancel button and a Connect button. The quick settings window
takes an exclusive keyboard grab, so a separate dialog cannot receive typing. Do
not put the password in a tooltip or a log.

## CSS

Keep the `.QuickSettings-wifi`, `.network-item`, `.network-button`, `.network-name`, `.network-status`, `.connected`, and `.empty-state` rules. Keep the `.large-button` and `.dropdown` rules. Replace only the dropdown animation mechanism, not the visual classes.

## Definition of done

- The bar Wi-Fi icon and tooltip follow the active SSID and signal.
- The bar widget hides when no Wi-Fi device exists and returns after hot-plug.
- The quick settings button shows `WiFi Off` or the SSID.
- The dropdown lists one row per network, not one per BSSID.
- The list sorts by signal strength and updates on scan.
- Clicking a saved network connects; clicking the active network disconnects.
- A secured network without a profile opens a password dialog and connects with the correct security setting.
- An enterprise network without a profile shows a clear message instead of failing silently.
- The switch enables and disables Wi-Fi.
- A null BSSID never crashes or loses the active access point.
- No NetworkManager, libnm, or blocking D-Bus call runs on the GTK thread.

## Wired, VPN, and airplane mode

These extend the model above in the Rust rewrite.

### Wired

`WiredDeviceModel` carries the interface name, `DeviceState`, carrier, connection
name, IPv4 address, and speed. `enabled` is user intent, not the device state, so
an unplugged cable does not read as "off". Turning an interface off calls
`Device.Disconnect`; turning it on activates the first available connection.
Wired interfaces have no rfkill entry and airplane mode never touches them.

### VPN

VPN becomes part of `NetworkService`, following PR #478. `VpnModel` lists the
NetworkManager profiles whose type is `vpn` or `wireguard`, keyed by UUID, with
`active` and `activation_failed`. Activation calls `ActivateConnection` with a
null device; deactivation finds the active connection by UUID. The quick
settings VPN toggle opens its own dropdown; the network dropdown only lists
wired and Wi-Fi devices. Each dropdown opens inline after its own toggle row, so
the rows below move down while it is open.

A tunnel can be active without NetworkManager having started it, for example a
WireGuard interface created by `wg-quick`. NetworkManager marks those active
connections with `NM_ACTIVATION_STATE_FLAG_EXTERNAL`. The shell shows them as
`External`. Stopping one asks for confirmation first: NM did not create it, so it
cannot start it again, and the owner's systemd unit may need a restart. After the
user confirms, the shell sends the normal deactivation.

### Airplane mode

`AirplaneModeSet(bool)` does two things:

1. Saves `WirelessEnabled`, `WwanEnabled`, and every rfkill soft state, then sets
   the NetworkManager properties false and soft-blocks every wireless rfkill radio.
   The block goes through `/dev/rfkill`, which the session owner can usually
   write, and falls back to the sysfs `soft` attribute.
2. On disable, restores the saved NetworkManager flags and rfkill states.

Bluetooth is covered by its rfkill radio now, before a Bluetooth service exists.
Wired devices are excluded by construction.

### Power saving

The service never polls and never scans on a timer.

- State arrives from D-Bus signals, and a burst of signals collapses into one
  read after 300 ms.
- The access point list is loaded only after `WifiScan` or `WifiSetDetails(true)`.
  The quick settings dropdown sends both when it opens and
  `WifiSetDetails(false)` when it closes.
- `RequestScan` runs only on `WifiScan`, sent by the dropdown open handler and
  the refresh button.
