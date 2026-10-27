# Quick settings

Current files: `src/quick_settings/Window.vala`, `widgets/Card.vala`, `Connectivity.vala`, `LargeButton.vala`, `QuickToggleButton.vala`, `VolumeSlider.vala`, `Microphone.vala`, `BrightnessSlider.vala`, `QuickSettings.scss`.

The network, Bluetooth, and KDE Connect dropdowns have their own files. Read `features/network.md`, `features/bluetooth.md`, and `features/kde-connect.md` with this one.

## Current behavior

### Window

`QuickSettings.Window` is an `Astal.Window`:

- Anchor top, bottom, left, right.
- Exclusivity `IGNORE`.
- Layer `OVERLAY`.
- Keymode `EXCLUSIVE`.
- Visible false.
- Namespace `nosh-quick-settings`.
- CSS class `QuickSettings`.

The window contains a vertical `Gtk.Box` with class `background`, expanding in both directions. The background is clickable: if a click hits the background itself, the window closes. An Escape key (`0xffc1` or `65307`) closes it. The window binds `AppState.quick_settings_open` to `visible` bidirectionally. When visible, it calls `present()` and `grab_focus()`.

### Card

`Card` is a vertical `Gtk.Box` with class `card`:

- Align end and start.
- No vertical expand.
- Margins: top 56, end 16, bottom 16.
- Contains a `Gtk.ScrolledWindow` with no scrollbars and minimum width 260.

Content, top to bottom:

1. `Connectivity`.
2. Divider.
3. Sliders: `VolumeSlider`, `Microphone`, `BrightnessSlider`.
4. Divider.
5. Toggle row: `DND`, `No Sleep`, `VPN`.

The toggle row is horizontal, spacing 8, classes `item-row` and `toggles`, fill, homogeneous.

### Large button

`LargeButton` is a vertical `Gtk.Box` containing a `Gtk.Button` with class `large-button`.

- Content: vertical box, centered, margins 8, spacing 4.
- Icon, size `LARGE`, class `icon`.
- Status label, class `status`, no wrap, no ellipsis, centered, max 10 characters.
- `set_icon_name`, `set_icon_enabled`, `set_status`.
- Long status text scrolls one character every 500 ms, 10 characters visible, then wraps to the start.

The scrolling text is a custom ticker. It is not a `Gtk.ScrolledWindow`.

### Quick toggle button

`QuickToggleButton` is a `Gtk.Button` with class `QuickSettings-toggle-button`.

- Stores `toggle_type` and `active`.
- Content: vertical box, centered, margins 8, spacing 4.
- Icon, size `LARGE`, class `icon`.
- Label, class `QuickSettings-label`, wrap, centered.
- Click flips `active` and adds or removes the `active` class.

The toggles:

| Type    | Icon                               | Label                                       |
| ------- | ---------------------------------- | ------------------------------------------- |
| `dnd`   | `notifications-disabled-symbolic`  | `DND`                                       |
| `sleep` | `eye-slash-symbolic`               | `No Sleep`                                  |
| `power` | `power-profile-{profile}-symbolic` | `Power Saver`, `Balanced`, or `Performance` |
| `vpn`   | `security-high-symbolic`           | `VPN`                                       |

All four currently only flip local state. DND does not change notification behavior. Sleep does not toggle the idle inhibitor. VPN does nothing. The power control follows `features/power-profiles.md`.

Decision: DND and No Sleep are wired to the core. Power profiles are wired when the daemon is present. VPN stays disabled until a VPN service exists.

### Sliders

- Volume: 0 to 150, handled in `features/audio.md`.
- Microphone: 0 to 100, handled in `features/audio.md`.
- Brightness: 0 to 100, handled in `features/brightness.md`.

### Dropdown animation

`Connectivity.toggle_dropdown` implements a max-height animation:

- On close, read the current height, load `* { max-height: <height>px !important; }` into a shared CSS provider, add class `height-limit`, remove class `visible`, then after 10 ms set max-height to 0, and after 310 ms set `visible = false`.
- On open, set `visible = true`, then in an idle callback measure the natural height, load `max-height: <height + 50>px`, add `height-limit`, and add class `visible`.

This is a workaround for GTK CSS transitions. It uses a single provider for every dropdown and is fragile.

Replace it with `gtk::Revealer`:

- `set_transition_type(RevealerTransitionType::SlideDown)`.
- `set_transition_duration(300)`.
- `set_reveal_child(true/false)`.
- Keep only one dropdown open at a time.
- Close all dropdowns when the quick settings window closes.
- Do not use a shared CSS provider for animation.

## Core

The quick settings window is UI. It sends commands for the services:

- `AudioSetVolume`, `AudioSetSourceVolume`, `AudioSetMuted`.
- `BrightnessSet`.
- `WifiSetEnabled`, `WifiScan`, `WifiActivate`, `WifiDeactivate`.
- `BluetoothSetPowered`, `BluetoothConnect`, `BluetoothDisconnect`, `BluetoothStartDiscovery`.
- `NotificationSetDnd`.
- `IdleInhibitSet`.
- `PowerProfileSet`, `PowerProfileCycle`.
- KDE Connect commands for daemon state, device refresh, clipboard, and file send.

The UI owns `quick_settings_open`. The core owns the actual system states.

### Toggles

Wire DND and No Sleep:

- DND reads `NotificationModel.dont_disturb` and sends `NotificationSetDnd(!dnd)`.
- No Sleep reads `IdleInhibitModel.active` and sends `IdleInhibitSet(!active)`.
- Power cycles through the available profiles from `PowerProfilesService` and sends `PowerProfileSet(next)`.
- VPN stays disabled until a `VpnService` exists. Show a tooltip explaining that VPN support is not available. Do not flip a local boolean.

The toggle button should show its `active` class from the core state, not from a local click. Clicking sends a command and waits for the event. This prevents the button from lying when the service fails.

### Dropdown state

Keep one enum in the UI:

```rust
enum OpenDropdown {
    None,
    Wifi,
    Bluetooth,
    KdeConnect,
}
```

Opening one closes the others. Closing quick settings sets it to `None`. Do not use `visible` on multiple widgets as the source of truth.

Opening the Bluetooth dropdown sends `BluetoothStartDiscovery`. Opening the Wi-Fi dropdown may start a scan if the access point list is stale. Both actions belong in the dropdown's open handler, not in its constructor.

## UI

### Window

Create a `gtk::Window` with `gtk4_layer_shell`:

- All four anchors.
- Layer `Overlay`.
- Exclusivity `Ignore`.
- Keyboard mode `Exclusive`.
- Namespace `nosh-quick-settings`.
- Monitor: the current monitor. Set `gtk4_layer_shell::set_monitor` before presenting. If the window is already mapped, hide it, set the monitor, then present.

Set `decorated(false)` and `resizable(false)`. Add class `QuickSettings`.

The background box is a `gtk::Box` with class `background`. Add a `gtk::GestureClick`. On press, if `box.pick(x, y, PickFlags::DEFAULT) == Some(box.clone().upcast())`, close. The exact pick comparison is easier with `Widget::pick` returning a widget; compare pointers or `WidgetExt::is_ancestor`.

Add a `gtk::EventControllerKey` on the window. Escape closes. Use `gdk::Key::Escape` instead of numeric key values.

When the window becomes visible, call `present()` and `grab_focus()` on the card or the first focusable child. When it hides, close dropdowns and clear focus.

Bind visibility to the UI `quick_settings_open` boolean. Use a single function `set_quick_settings_open(bool)` that updates the window, the button state, and the dropdown state.

### Card

Reuse `Card` with the current CSS and margins. Use a `gtk::ScrolledWindow` only if the content can exceed the screen. The current policy is `NEVER` for both axes, so the scrolled window does nothing. If the card fits, a plain `gtk::Box` is simpler. Keep the minimum width 260.

### Large button

Reuse `LargeButton`:

- Icon, status label, optional label.
- `set_icon_enabled` toggles `enabled` and `disabled` classes on the icon.
- `set_status` sets the text.
- The character ticker is optional. If the status is longer than 10 characters, use `Pango::EllipsizeMode::End` and a tooltip instead of a timer. The current ticker is a visual quirk, not a requirement. If you keep it, use a `glib::timeout_add_local` bound to the widget and stop it when the text becomes short or the widget is unparented.

### Toggles

Use a `gtk::Button` with a vertical icon and label like `QuickToggleButton`. Keep classes `QuickSettings-toggle-button`, `icon`, `QuickSettings-label`, and `active`.

Do not use `Gtk.ToggleButton` unless its default styling is fully overridden. The current code uses a plain button and a bool.

### Sliders

Use the volume, microphone, and brightness widgets from their feature files. Keep the header layout and classes.

### Layout

```text
background
  card
    connectivity row
    divider
    volume
    microphone
    brightness
    divider
    toggles
      DND
      No Sleep
      Power profile
      VPN
```

The card is top-right. The current margin top is 56, which leaves room for the bar and a small gap. Keep it unless the bar height changes.

## CSS

Keep the existing `QuickSettings.scss` selectors:

- `.background` `rgba(0,0,0,0.15)`.
- `.card` background `@window_bg_color`, radius 12, border `rgba(255,255,255,0.1)`, shadow.
- `.item-row` padding 8 12.
- `.dropdowns-container` padding 8 12.
- `.divider` margin 6 0, background `rgba(255,255,255,0.08)`.
- `.toggles`, `.large-button`, `.dropdown`, `.slider`, `.volume`, and the KDE Connect rules.

Do not keep the max-height animation CSS. Keep the visual result of a 300 ms slide.

## Definition of done

- The window maps full-screen on the overlay layer with keyboard exclusive mode and namespace `nosh-quick-settings`.
- Escape and a click on the background close it.
- The card sits at the top-right with the current margins and minimum width.
- The window opens on the current monitor, not always the primary.
- Connectivity, volume, microphone, brightness, and the four toggles appear in the current order.
- DND controls notification popups.
- No Sleep controls the idle inhibitor.
- The power control changes the active UPower power profile and shows the current one.
- VPN is visibly disabled until a VPN service exists.
- Opening one dropdown closes the other two.
- Closing quick settings closes all dropdowns.
- Dropdowns animate with a `Revealer`, not a shared CSS max-height hack.
- No widget sends a command directly to D-Bus or Hyprland.
