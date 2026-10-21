# D-Bus interfaces

Pseudocode summaries for the D-Bus services the shell uses. Names and signatures are functional facts. Verify exact signatures and permissions against the installed service version.

Conventions:

```text
s   string
o   object path
b   boolean
u   uint32
y   byte
i   int32
x   int64
d   double
as  array of strings
ao  array of object paths
ay  byte array
a{sv} dictionary string -> variant
a{ss} dictionary string -> string
```

## NetworkManager

System bus.

```text
service org.freedesktop.NetworkManager
object /org/freedesktop/NetworkManager
  interface org.freedesktop.NetworkManager
    property Devices: ao
    property WirelessEnabled: b
    property State: u
    property Connectivity: u
    property PrimaryConnection: o
    property ActivatingConnection: o

    method GetDevices() -> ao
    method ActivateConnection(connection: o, device: o, specific: o) -> o
    method AddAndActivateConnection(settings: a{sa{sv}}, device: o, specific: o)
        -> (path: o, active_connection: o)
    method DeactivateConnection(active_connection: o)
    method CheckConnectivity() -> u

    signal DeviceAdded(device: o)
    signal DeviceRemoved(device: o)
    signal StateChanged(state: u)
    signal PropertiesChanged(interface: s, changed: a{sv}, invalidated: as)
```

Device:

```text
interface org.freedesktop.NetworkManager.Device
  property DeviceType: u
  property State: u
  property ActiveConnection: o
  property Interface: s
  property IpInterface: s
  property Managed: b

  signal StateChanged(new_state: u, old_state: u, reason: u)
```

Device types used here:

```text
2  Wi-Fi
1  Ethernet
```

Wi-Fi device:

```text
interface org.freedesktop.NetworkManager.Device.Wireless
  property AccessPoints: ao
  property ActiveAccessPoint: o
  property LastScan: x

  method RequestScan(options: a{sv})
  method GetAccessPoints() -> ao
  method GetAllAccessPoints() -> ao

  signal AccessPointAdded(access_point: o)
  signal AccessPointRemoved(access_point: o)
```

Access point:

```text
interface org.freedesktop.NetworkManager.AccessPoint
  property Ssid: ay
  property Bssid: s
  property Strength: y
  property Frequency: u
  property Bandwidth: u
  property MaxBitrate: u
  property Mode: u
  property Flags: u
  property WpaFlags: u
  property RsnFlags: u
  property LastSeen: i
```

Scanning pseudocode:

```text
scan(wifi_device):
  options = {}
  call RequestScan(options)
  mark scanning = true
  timeout after 10 seconds:
    scanning = false
  when LastScan changes:
    scanning = false
    refresh AccessPoints
```

Activation pseudocode:

```text
activate(ssid, password, bssid):
  profiles = Settings.Connections
  matching = profiles where
    connection.type == "802-11-wireless"
    wireless.ssid == bytes(ssid)
    security type matches the AP

  if matching is not empty:
    profile = first(matching)
    if password is not null:
      update profile security with password
    call ActivateConnection(profile, wifi_device, ap_path)
    return

  if security is NONE:
    settings = base_settings(ssid)
  else if security is OWE:
    settings = base_settings(ssid)
    settings["802-11-wireless-security"] = { "key-mgmt": "owe" }
  else if security is STATIC_WEP:
    settings = base_settings(ssid)
    settings["802-11-wireless-security"] = {
      "key-mgmt": "none",
      "wep-key-type": "passphrase",
      "wep-key0": password
    }
  else if security is WPA_PSK or WPA2_PSK:
    settings = base_settings(ssid)
    settings["802-11-wireless-security"] = {
      "key-mgmt": "wpa-psk",
      "psk": password
    }
  else if security is SAE:
    settings = base_settings(ssid)
    settings["802-11-wireless-security"] = {
      "key-mgmt": "sae",
      "psk": password
    }
  else:
    error "enterprise networks need an existing 802.1X profile"

  call AddAndActivateConnection(settings, wifi_device, ap_path)
```

Base settings pseudocode:

```text
base_settings(ssid):
  {
    "connection": {
      "id": ssid,
      "type": "802-11-wireless",
      "uuid": new_uuid()
    },
    "802-11-wireless": {
      "ssid": bytes(ssid),
      "mode": "infrastructure"
    },
    "ipv4": { "method": "auto" },
    "ipv6": { "method": "auto" }
  }
```

Security detection order, strongest first:

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

Pick the first type that the Wi-Fi device capabilities and the AP flags allow. If none is valid, the device cannot connect to that AP.

Do not pin a BSSID for a grouped network. Pinning stops roaming between access points that share an SSID and security type.

## BlueZ

System bus, service `org.bluez`, root path `/`.

```text
interface org.freedesktop.DBus.ObjectManager
  method GetManagedObjects() -> a{oa{sa{sv}}}
  signal InterfacesAdded(path: o, interfaces: a{sa{sv}})
  signal InterfacesRemoved(path: o, interfaces: as)
```

Adapter:

```text
interface org.bluez.Adapter1
  property Powered: b
  property Discoverable: b
  property Pairable: b
  property Discovering: b
  property Alias: s
  property Name: s
  property Address: s
  property UUIDs: as
  property DiscoverableTimeout: u
  property PairableTimeout: u
  property Modalias: s
  property Class: u

  method StartDiscovery()
  method StopDiscovery()
  method RemoveDevice(device: o)
```

Device:

```text
interface org.bluez.Device1
  property Connected: b
  property Paired: b
  property Trusted: b
  property Blocked: b
  property Alias: s
  property Name: s
  property Address: s
  property Icon: s
  property RSSI: i16
  property Adapter: o
  property UUIDs: as
  property Appearance: u16
  property Class: u32
  property LegacyPairing: b
  property Modalias: s

  method Connect()
  method Disconnect()
  method Pair()
  method CancelPairing()
  method ConnectProfile(uuid: s)
  method DisconnectProfile(uuid: s)
```

Battery:

```text
interface org.bluez.Battery1
  property Percentage: y
  property Source: s
```

Agent manager and agent:

```text
interface org.bluez.AgentManager1
  method RegisterAgent(agent: o, capability: s)
  method UnregisterAgent(agent: o)
  method RequestDefaultAgent(agent: o)
```

Capabilities: `DisplayOnly`, `DisplayYesNo`, `KeyboardOnly`, `KeyboardDisplay`, `NoInputNoOutput`.

```text
interface org.bluez.Agent1
  method Release()
  method RequestPinCode(device: o) -> s
  method DisplayPinCode(device: o, pincode: s)
  method RequestPasskey(device: o) -> u
  method DisplayPasskey(device: o, passkey: u, entered: u16)
  method RequestConfirmation(device: o, passkey: u)
  method RequestAuthorization(device: o)
  method AuthorizeService(device: o, uuid: s)
  method Cancel()
```

Pairing pseudocode:

```text
pair(device_path):
  show confirmation dialog when RequestConfirmation arrives
  accept RequestAuthorization only after the user confirms
  return the entered PIN or passkey for RequestPinCode and RequestPasskey
  on Cancel, reject the operation and show one error
  never auto-trust
```

Discovery pseudocode:

```text
open_dropdown:
  call Adapter1.StartDiscovery()
  mark discovering = true
  timeout 30 seconds:
    call Adapter1.StopDiscovery()
    discovering = false

refresh:
  call Adapter1.StartDiscovery()
  restart the timeout
```

Handle interfaces in this order: `Adapter1`, `Device1`, then `Battery1`. A `Battery1` that arrives before its `Device1` would otherwise be dropped.

## UPower

System bus.

```text
service org.freedesktop.UPower
object /org/freedesktop/UPower
  interface org.freedesktop.UPower
    property DaemonVersion: s
    property OnBattery: b
    property LidIsClosed: b
    property LidIsPresent: b

    method EnumerateDevices() -> ao
    method GetDisplayDevice() -> o

    signal DeviceAdded(device: o)
    signal DeviceRemoved(device: o)
```

Display device:

```text
object /org/freedesktop/UPower/devices/DisplayDevice
  interface org.freedesktop.UPower.Device
    property Type: u
    property Percentage: d
    property State: u
    property IsPresent: b
    property PowerSupply: b
    property TimeToEmpty: x
    property TimeToFull: x
    property Energy: d
    property EnergyFull: d
    property EnergyRate: d
    property IconName: s
    property WarningLevel: u
    property BatteryLevel: u
    property NativePath: s
    property Vendor: s
    property Model: s
    property Serial: s
```

Types and states:

```text
Type:
  1 line power
  2 battery

State:
  1 charging
  2 discharging
  3 empty
  4 fully charged
  5 pending charge
  6 pending discharge
```

UPower returns percentage as `0..100`. Normalize to `0..1` in the core.

## PowerProfiles

System bus.

```text
service org.freedesktop.UPower.PowerProfiles
object /org/freedesktop/UPower/PowerProfiles
  interface org.freedesktop.UPower.PowerProfiles
    property ActiveProfile: s
    property Profiles: aa{sv}
    property Actions: as
    property ActiveProfileHolds: aa{sv}
    property PerformanceDegraded: s
    property Version: s

    method HoldProfile(profile: s, reason: s, application_id: s) -> u
    method ReleaseProfile(cookie: u)

    signal ProfileReleased(cookie: u)
```

Profile strings: `power-saver`, `balanced`, `performance`.

## MPRIS

Session bus. Service names start with `org.mpris.MediaPlayer2.`. Object path `/org/mpris/MediaPlayer2`.

```text
interface org.mpris.MediaPlayer2
  property Identity: s
  property DesktopEntry: s
  property CanQuit: b
  property CanRaise: b
  property CanSetFullscreen: b
  property Fullscreen: b
  property SupportedUriSchemes: as
  property SupportedMimeTypes: as

  method Raise()
  method Quit()
```

```text
interface org.mpris.MediaPlayer2.Player
  property PlaybackStatus: s
  property LoopStatus: s
  property Rate: d
  property Shuffle: b
  property Metadata: a{sv}
  property Volume: d
  property Position: x
  property MinimumRate: d
  property MaximumRate: d
  property CanGoNext: b
  property CanGoPrevious: b
  property CanPlay: b
  property CanPause: b
  property CanSeek: b
  property CanControl: b

  method Next()
  method Previous()
  method Pause()
  method PlayPause()
  method Stop()
  method Play()
  method Seek(offset: x)
  method SetPosition(track_id: o, position: x)
  method OpenUri(uri: s)

  signal Seeked(position: x)
```

Metadata keys used by the bar:

```text
mpris:trackid       object path or string
mpris:length        int64 microseconds
mpris:artUrl        string
xesam:title         string
xesam:album         string
xesam:artist        array<string>
xesam:albumArtist   array<string>
xesam:asText        string
xesam:comments      array<string>
xesam:composer      array<string>
```

Join artist and album artist with `", "`. Join comments with `"\n"`.

Position is in microseconds. The `Position` property does not emit `PropertiesChanged`. Poll only while the player is playing and the UI needs progress. `Seeked` gives a new position.

## Notifications

Session bus. The shell can own this name or proxy an existing daemon.

```text
service org.freedesktop.Notifications
object /org/freedesktop/Notifications
  interface org.freedesktop.Notifications
    method Notify(
      app_name: s,
      replaces_id: u,
      app_icon: s,
      summary: s,
      body: s,
      actions: as,
      hints: a{sv},
      expire_timeout: i
    ) -> u

    method CloseNotification(id: u)
    method GetCapabilities() -> as
    method GetServerInformation() -> (name: s, vendor: s, version: s, spec_version: s)

    signal NotificationClosed(id: u, reason: u)
    signal ActionInvoked(id: u, action_key: s)
    signal ActivationToken(id: u, token: s)
```

Closed reasons:

```text
1 expired
2 dismissed by user
3 closed by CloseNotification
4 undefined
```

Standard hints:

```text
action-icons       b
category           s
desktop-entry      s
image-path         s
image-data         (iiibiiay)  width, height, rowstride, has_alpha, bits_per_sample, channels, data
sound-file         s
sound-name         s
suppress-sound     b
resident           b
transient          b
urgency            y            0 low, 1 normal, 2 critical
x                  i
y                  i
```

Actions are a flat array of pairs: `id1, label1, id2, label2`. Ignore a trailing unmatched string.

Notify pseudocode:

```text
notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout):
  if hints has "image-data":
    save the image as a cached PNG
    hints["image-path"] = cached path
    remove "image-data"
  remove "image_data" and "icon_data"
  if replaces_id exists:
    id = replaces_id
    replaced = true
  else:
    id = next_id()
    replaced = false
  timeout = expire_timeout >= 0 ? expire_timeout : default_timeout
  store notification
  emit notified(id, replaced)
  schedule close after timeout when timeout > 0 and ignore_timeout is false
  return id
```

## StatusNotifier

Session bus. The shell can own the watcher or proxy an existing one.

```text
service org.kde.StatusNotifierWatcher
object /StatusNotifierWatcher
  interface org.kde.StatusNotifierWatcher
    property RegisteredStatusNotifierItems: as
    property IsStatusNotifierHostRegistered: b
    property ProtocolVersion: i

    method RegisterStatusNotifierItem(service: s)
    method RegisterStatusNotifierHost(service: s)

    signal StatusNotifierItemRegistered(service: s)
    signal StatusNotifierItemUnregistered(service: s)
    signal StatusNotifierHostRegistered()
    signal StatusNotifierHostUnregistered()
```

Registration parsing:

```text
if service starts with "/":
  bus_name = sender
  object_path = service
else if service contains "/":
  bus_name = part before the first "/"
  object_path = "/" + part after the first "/"
else:
  bus_name = service
  object_path = "/StatusNotifierItem"
```

Item:

```text
interface org.kde.StatusNotifierItem
  property Id: s
  property Category: s
  property Title: s
  property Status: s
  property ToolTip: (s, a(iiay), s, s)  icon name, icon pixmaps, title, description
  property IconThemePath: s
  property ItemIsMenu: b
  property Menu: o
  property IconName: s
  property IconPixmap: a(iiay)
  property AttentionIconName: s
  property AttentionIconPixmap: a(iiay)
  property OverlayIconName: s
  property OverlayIconPixmap: a(iiay)

  method ContextMenu(x: i, y: i)
  method Activate(x: i, y: i)
  method SecondaryActivate(x: i, y: i)
  method Scroll(delta: i, orientation: s)

  signal NewTitle()
  signal NewIcon()
  signal NewAttentionIcon()
  signal NewOverlayIcon()
  signal NewToolTip()
  signal NewStatus(status: s)
```

Pixmaps are `(width, height, bytes)`. Bytes are ARGB32 in network byte order. Convert to RGBA before creating a texture. Reject width or height <= 0.

## DBusMenu

Session bus, at the item's `Menu` path.

```text
interface com.canonical.dbusmenu
  property Version: u
  property TextDirection: s
  property Status: s
  property IconThemePath: as

  method GetLayout(parent_id: i, recursion_depth: i, property_names: as)
      -> (revision: u, layout: (id: i, properties: a{sv}, children: av))
  method GetGroupProperties(ids: ai, property_names: as)
      -> a(ia{sv})
  method GetProperty(id: i, name: s) -> v
  method Event(id: i, event_id: s, data: v, timestamp: u)
  method EventGroup(events: a(isvu))
  method AboutToShow(id: i) -> b
  method AboutToShowGroup(ids: ai) -> (updates_needed: ai, id_errors: ai)

  signal ItemsPropertiesUpdated(updated: a(ia{sv}), removed: a(ias))
  signal LayoutUpdated(revision: u, parent: i)
  signal ItemActivationRequested(id: i, timestamp: u)
```

Item properties used:

```text
label             s
enabled           b
visible           b
type              s    "standard", "separator"
children-display  s    "submenu"
toggle-type       s    "checkmark", "radio"
toggle-state      i    0 off, 1 on, 2 indeterminate
icon-name         s
shortcut          as
```

Menu pseudocode:

```text
open_menu(item):
  call AboutToShow(0)
  layout = GetLayout(0, -1, wanted_properties)
  build_menu(layout)
  rebuild when LayoutUpdated fires
  update labels and states when ItemsPropertiesUpdated fires

click(id):
  call Event(id, "clicked", variant, current_time)
```

## KDE Connect

Session bus.

```text
service org.kde.kdeconnect
object /modules/kdeconnect
  interface org.kde.kdeconnect.daemon
    method devices(reachable: b, paired: b) -> as
    signal device_added(id: s)
    signal device_removed(id: s)
    signal device_visibility_changed(id: s, visible: b)
```

```text
object /modules/kdeconnect/devices/<id>
  interface org.kde.kdeconnect.device
    property id: s
    property name: s
    property isReachable: b
    property isTrusted: b
    property charge: i
    property isCharging: b

    method isPluginEnabled(plugin: s) -> b
    method setPluginEnabled(plugin: s, enabled: b)
```

```text
object /modules/kdeconnect/devices/<id>/clipboard
  interface org.kde.kdeconnect.device.clipboard
    method sendClipboard()
```

```text
object /modules/kdeconnect/devices/<id>/share
  interface org.kde.kdeconnect.device.share
    method shareUrls(urls: as)
```

Use `NameOwnerChanged` for `org.kde.kdeconnect` instead of polling. Start and stop the daemon through its systemd user unit. Do not use `pkill` by default.

## logind

System bus.

```text
service org.freedesktop.login1
object /org/freedesktop/login1/session/auto
  interface org.freedesktop.login1.Session
    method SetBrightness(subsystem: s, name: s, brightness: u)
    method Lock()
    method Unlock()
    method Terminate()
```

```text
object /org/freedesktop/login1
  interface org.freedesktop.login1.Manager
    method PowerOff(interactive: b)
    method Reboot(interactive: b)
    method Suspend(interactive: b)
    method Hibernate(interactive: b)
    method HybridSleep(interactive: b)
    method LockSessions()
    method ListSessions() -> a(susso)
    method TerminateSession(session_id: s)
```

`SetBrightness` uses subsystem `"backlight"` for the internal panel and the sysfs device name as `name`. The brightness value is the raw value, not a percentage.

Power actions need the correct polkit authorization. Handle `AccessDenied` and show the error instead of retrying in a loop.
