# Battery

Current file: `src/bar/widgets/Battery.vala`.

## Current behavior

`Battery` is a `Gtk.Button` with CSS class `square-icon`, centered, containing a `Gtk.Image`.

It gets `AstalBattery.get_default()` and binds `battery-icon-name` to the image `icon-name`. It sets a tooltip `Battery: N%` from `Math.floor(percentage * 100)` and updates it when `percentage` changes.

`AstalBattery` uses the UPower `DisplayDevice` at `/org/freedesktop/UPower/devices/DisplayDevice`. Its `percentage` is normalized to `0..1`. The icon is computed in `Device.sync()`:

```text
if not a battery: "battery-missing-symbolic"
else if percentage >= 0.95 and charging: "battery-level-100-charged-symbolic"
else: "battery-level-{round(percentage * 10) * 10}{-charging}-symbolic"
```

`charging` is true for `CHARGING` or `FULLY_CHARGED`.

The widget is always visible, even on a desktop with no battery.

## Core

Add a `BatteryService` to `nosh-core`.

### D-Bus

System bus, `org.freedesktop.UPower`, path `/org/freedesktop/UPower/devices/DisplayDevice`, interface `org.freedesktop.UPower.Device`.

Read:

- `Type` (`uint`; `2` is battery, `1` is line power).
- `Percentage` (`double`, `0..100`).
- `State` (`uint`; `1` charging, `2` discharging, `3` empty, `4` fully charged, `5` pending charge, `6` pending discharge).
- `IsPresent` (`bool`).
- `PowerSupply` (`bool`).
- `TimeToEmpty`, `TimeToFull` (`i64` seconds).
- `Energy`, `EnergyFull`, `EnergyRate`.
- `IconName` (`string`).
- `WarningLevel`, `BatteryLevel`.
- `NativePath`, `Vendor`, `Model`, `Serial`.

Subscribe to `PropertiesChanged`. Emit a full snapshot on start and a changed event on every relevant property.

If the DisplayDevice path does not exist, fall back to `org.freedesktop.UPower.EnumerateDevices` and choose the first device with `Type == 2` and `PowerSupply == true`. If none exists, emit `BatteryEvent::Unavailable`.

### Model

```rust
pub struct BatteryModel {
    pub present: bool,
    pub is_battery: bool,
    pub percentage: f64, // 0.0..=1.0
    pub state: BatteryState,
    pub charging: bool,
    pub icon_name: String,
    pub time_to_empty: Option<Duration>,
    pub time_to_full: Option<Duration>,
}
```

Normalize `Percentage` by dividing by 100. Keep the clamped value.

### Icon

Use the exact rule above. Compute it in the core so the UI does no string formatting:

```rust
fn icon_name(model: &BatteryModel) -> String {
    if !model.is_battery {
        "battery-missing-symbolic".into()
    } else if model.percentage >= 0.95 && model.charging {
        "battery-level-100-charged-symbolic".into()
    } else {
        let level = (model.percentage * 10.0).round() as i32 * 10;
        let suffix = if model.charging { "-charging" } else { "" };
        format!("battery-level-{level}{suffix}-symbolic")
    }
}
```

Round before multiplying. `0.949` rounds to 9, giving 90. `0.95` rounds to 10, giving 100. The `>= 0.95 && charging` branch catches the full case first.

### Tooltip

Format `Battery: {floor(percentage * 100)}%`. Use `floor`, not `round`, for parity.

If `time_to_empty` or `time_to_full` is available and the user wants a richer tooltip, add it later. Do not change the default text.

## UI

`Battery` is a `gtk::Button` with class `square-icon`, a centered `gtk::Image` child, and a tooltip.

- Bind the image `icon-name` to `BatteryModel.icon_name`.
- Bind the tooltip to `Battery: N%`.
- Hide the button when `BatteryModel.present` is false or `is_battery` is false. The current code shows a missing-battery icon; hiding is cleaner on desktops. This is a small behavior change. If exact parity matters, keep the missing icon.
- Do not create a new image on every update.

If the battery is critical, add a CSS class such as `critical` only if the design asks for it. The current code does not.

## CSS

`.square-icon` already gives the 32x32 button and 30x30 image. Do not add a new class.

## Definition of done

- A laptop battery shows an icon and a tooltip with the floored percentage.
- Charging and fully charged use the correct icon suffix.
- A desktop with no battery hides the widget, or shows `battery-missing-symbolic` if parity is chosen.
- UPower restart reconnects and emits a fresh snapshot.
- The core normalizes percentage to `0..1`; the UI never divides by 100.
- The icon and tooltip update without recreating the widget.
