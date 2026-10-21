# Brightness

Current files: `src/utils/system_control/BrightnessControl.vala`, `src/quick_settings/widgets/BrightnessSlider.vala`, `src/indicators/ChangeIndicator.vala`.

## Current behavior

There are two independent brightness paths.

### System control

`BrightnessControl` shells out to `brightnessctl --exponent=2.0`:

- `increase(step = 5)` runs `brightnessctl --exponent=2.0 set +5%`.
- `decrease(step = 5)` runs `brightnessctl --exponent=2.0 set 5%-`.
- `set(value)` clamps to `0..100` and runs `brightnessctl --exponent=2.0 set <value>%`.
- `get()` runs `brightnessctl --exponent=2.0 -m` and parses the fourth comma-separated field, removing `%`.
- Each successful write emits `brightness_changed(new, old)`.

### Slider

`BrightnessSlider` uses `light`:

- `light -G` reads the brightness as an integer.
- `light -S <value>` sets it.
- The setter runs in a new thread and immediately joins it, which blocks the caller.
- The scale range is `0..100`, step 1.
- The percentage label is the integer scale value.

The two paths can disagree, and both `light` and `brightnessctl` must be installed.

### OSD

`ChangeIndicator.show_brightness_indicator` divides brightness by 100, labels `"{brightness}%"`, and shows the indicator for 800 ms.

### External displays

The current code does not control external monitors. The user has a separate script that saves the internal backlight and every DDC/CI monitor, dims them to 10, and restores them later. The rewrite must support that model.

## Core

Add a `BrightnessService` to `nosh-core`. It is an actor, not a direct call path. All backends run behind a command queue so a slow DDC/CI call cannot block the GTK thread, the Tokio runtime, or another display.

Backend resolution is a per-device try tree, not one global backend. See [backends.md](../backends.md).

| Device | Try order |
| --- | --- |
| Internal panel | `SysfsLogindBackend`, then `BrightnessctlBackend`, then `CommandBackend` |
| External display | `DdcutilBackend`, then `DdcHiBackend`, then `CommandBackend` |
| Master control | `CompositeBackend`, or `SingleDeviceBackend` when only one device exists |

Resolve each device independently. One monitor can fail DDC and still leave the internal panel and the other monitors working. The actor owns the queue above the backends; it is not itself a backend.

### Actor model

```text
UI or CLI
  -> mpsc/bounded queue
  -> brightness actor
       -> internal backend (fast, sysfs)
       -> external backend (slow, DDC/CI)
       -> command backend (user scripts)
  -> watch channel / Event stream
  -> UI
```

Rules:

- The actor owns the device list and the last known value for each device.
- A `Set` command replaces any pending `Set` for the same target. Only the newest value matters.
- A `Step` command is converted to an absolute target before it enters the queue.
- `Snapshot`, `Dim`, and `Restore` are serialized and must complete before later sets apply.
- The UI may update optimistically with a `pending` value, but the final state comes from the actor.
- A slow backend gets its own worker thread or `spawn_blocking` task. Do not call DDC/CI on the runtime thread.
- The actor emits a state update per device and a summary after a batch.

### Device model

```rust
pub struct BrightnessDevice {
    pub id: BrightnessDeviceId,
    pub kind: BrightnessKind,
    pub name: String,
    pub connector: Option<String>,
    pub percent: f64,          // 0.0..=100.0
    pub pending_percent: Option<f64>,
    pub available: bool,
    pub raw: Option<u32>,
    pub max_raw: Option<u32>,
    pub ddc_bus: Option<u8>,
}

pub enum BrightnessKind {
    Internal,
    ExternalDdc,
    ExternalCommand,
}

pub enum BrightnessDeviceId {
    Backlight(String),         // sysfs device name
    Ddc { bus: u8, display: String },
    Named(String),             // user command target
}
```

`connector` is the compositor output name when it can be matched, for example `DP-1`. `ddc_bus` is the I2C bus number. Persist the bus and connector mapping so a monitor keeps its identity across restarts.

### Internal backend

Preferred: sysfs plus logind.

- Discover backlights under `/sys/class/backlight/*`.
- Prefer `type == "raw"` over `"platform"` and `"firmware"`.
- Prefer a name matching `intel|amdgpu|radeon|nvidia`.
- Deprioritize names matching `acpi_video|video|firmware`.
- Read `brightness` and `max_brightness`.
- Write through `org.freedesktop.login1.Session.SetBrightness` on `/org/freedesktop/login1/session/auto`, interface `org.freedesktop.login1.Session`, signature `(ssu)`: subsystem `"backlight"`, device name, raw value.
- Watch the `brightness` file so external changes update the model.

Fallback: `brightnessctl --exponent=2.0`, used only when sysfs is unavailable. Keep the same commands as the current `BrightnessControl`.

Remove `light` completely.

### External backend

External monitors use DDC/CI VCP code `0x10` (brightness). Use a trait so the backend can change:

```rust
pub trait DdcBackend: Send + Sync {
    fn detect(&self) -> Result<Vec<DdcDisplay>, BrightnessError>;
    fn get(&self, display: &DdcDisplay) -> Result<u16, BrightnessError>;
    fn set(&self, display: &DdcDisplay, value: u16) -> Result<(), BrightnessError>;
}
```

Default implementation: `ddcutil`.

- `ddcutil detect` to list displays and buses.
- `ddcutil -b <bus> getvcp 10` to read the current and maximum value.
- `ddcutil -b <bus> setvcp 10 <value>` to write.
- Run each command with a timeout, default 5 seconds.
- Parse the current and max value. Do not rely on a percentage alone.
- Do not probe every monitor on startup if that adds several seconds. Detect in the background and add devices as they appear.

Optional implementation: `ddc-hi`.

- Direct I2C, lower latency.
- Less tolerant of monitor quirks than `ddcutil`.
- Use it only when the user selects it, or as a fast path after `ddcutil` has confirmed the bus and capabilities.
- Do not use both backends for the same display at once.

Why `ddcutil` is the default: it handles monitor quirks, multiple buses, and the many broken DDC implementations better than a direct I2C library. Since the service is an actor, the slower process call does not affect the UI.

Do not depend on `ddcutil` being installed. If it is missing, the external backend reports unavailable and the internal panel still works.

### Command backend

Some users have a script for a display that DDC cannot reach. Support a named command target:

```toml
[[brightness.device]]
name = "Desk lamp"
kind = "command"
get = "light -G"
set = "light -S {value}"
```

The `{value}` placeholder is replaced with an integer percentage. Run commands off the UI thread with a timeout. Use `sh -c` only because the user explicitly configured a command. Do not interpolate untrusted data.

This backend is also the escape hatch for a compositor-specific brightness tool.

### Gamma

The current `brightnessctl` calls use `--exponent=2.0`. If sysfs writes are direct, apply the same curve:

```text
raw = round(max_brightness * (percent / 100)^2)
percent = (raw / max_brightness)^(1/2) * 100
```

Keep `gamma = 2.0` in one constant. Test 0, 0.1, 0.5, and 1.0. DDC/CI uses the display's own scale, so do not apply the gamma curve to DDC values unless the user asks for it.

### Master and per-device control

- The quick settings slider controls all available displays together by default.
- The core sets each device to the same target percentage, clamped to its own range.
- A per-device popover or a future device selector can target one display.
- The OSD shows the master percentage.
- If one display fails, the others still change. The failed display is marked unavailable and retried later.

This matches the user's script, which applies the same dim value to every DDC bus and the internal panel.

### Save, dim, and restore

Implement the behavior of the user's script as first-class commands.

```rust
Command::BrightnessSnapshot
Command::BrightnessDim { value: f64 }
Command::BrightnessRestore
```

Snapshot:

- Read every available device.
- Store the internal device raw value or percentage and each DDC display's bus, connector, and value.
- Write `~/.cache/nosh/brightness/snapshot.json`.
- Use an atomic write with a temporary file and rename.

Example:

```json
{
  "created": 1700000000,
  "devices": [
    {"id": "backlight:intel_backlight", "kind": "internal", "percent": 62.0, "raw": 1240},
    {"id": "ddc:7:DP-1", "kind": "external", "bus": 7, "connector": "DP-1", "value": 80}
  ]
}
```

Dim:

- Snapshot first if no snapshot exists.
- Set every device to the configured dim value, default 10 percent.
- Serialize the writes. DDC writes are slow; run them one at a time or in a small pool, not all at once on the runtime thread.

Restore:

- Read the snapshot.
- Apply each entry to the matching device. Match by stable ID first, then by connector, then by DDC bus.
- If a device is missing, keep its entry and log once. Do not delete the snapshot.
- After a successful restore, remove the snapshot unless `keep_snapshot = true`.

This supports the `dim` and `restore` script behavior without shelling out to the user's script. The user can still configure a command device if a display needs special handling.

### Events

```rust
pub enum BrightnessEvent {
    Snapshot(Vec<BrightnessDevice>),
    DeviceChanged { device: BrightnessDeviceId, percent: f64, old: f64, source: ChangeOrigin },
    Pending { device: BrightnessDeviceId, target: f64 },
    Restored,
    Unavailable,
}
```

`ChangeOrigin` is `User`, `External`, `Initial`, or `Restore`. The change indicator shows only for `User`.

### Commands

```rust
Command::BrightnessSet(f64)                 // all devices
Command::BrightnessStep(f64)
Command::BrightnessSetTarget { device: BrightnessDeviceId, percent: f64 }
Command::BrightnessSnapshot
Command::BrightnessDim { value: f64 }
Command::BrightnessRestore
Command::BrightnessRescan
```

Add CLI commands:

```sh
nosh brightness-up
nosh brightness-down
nosh brightness-set 40
nosh brightness-dim
nosh brightness-restore
nosh brightness-snapshot
```

The exact names are part of the rewrite spec. Keep the existing `brightness-up` and `brightness-down` names for current keybinds.

### Discovery and hot-plug

- Scan internal backlights at startup and on `/sys/class/backlight` changes.
- Scan DDC displays at startup in the background and on monitor changes.
- When a monitor is added, try to detect its DDC bus.
- When a monitor is removed, keep its last value in the snapshot but mark the device unavailable.
- Do not block startup on DDC detection. The quick settings slider appears with the internal device and adds external devices when they are found.

## UI

### Brightness slider

`BrightnessSlider` is a vertical `gtk::Box` with class `slider`.

- Hide when no device is available.
- Header icon `display-brightness-symbolic`, label `Brightness`, percentage label with class `numeric`.
- `gtk::Scale` range `0..100`, step 1.
- On value change, send `BrightnessSet(value)`. Use a guard flag so a model update does not send a second command.
- Show the pending value immediately, then reconcile with the device event.
- Percentage label uses the integer master percent.
- Optional: a small disclosure or popover that lists each display and its value. Do not add it to the 260 px card in v1.

Do not create a thread in the UI. Do not join a thread. The core owns I/O.

### Change indicator

`ChangeIndicator` shows icon `display-brightness-symbolic`, label `"{percent}%"`, fraction `percent / 100`, and no overamp. See `features/change-indicator.md`.

### Failure state

If every brightness device fails, hide the slider and log once. If only external devices fail, keep the internal slider working and mark the external entries in the optional per-device popover.

## CSS

Keep the existing `.slider` rules and the `.indicator-bar` rules. No new class is needed for the master slider. A per-device popover can use the existing dropdown classes.

## Definition of done

- One backend selection path, with no `light` process anywhere.
- The internal panel works through sysfs plus logind, with `brightnessctl` as a fallback.
- External monitors work through DDC/CI VCP `0x10`.
- The slider and CLI control the internal panel and all detected external displays.
- A slow DDC call never blocks the GTK thread or the rest of the shell.
- Rapid slider movement coalesces to the latest value per device.
- `brightness-dim` saves the current state and sets every display to the dim value.
- `brightness-restore` restores every matching display and keeps missing entries.
- The snapshot is atomic, persisted, and survives a shell restart.
- External changes update the slider without showing the OSD.
- Shell-initiated changes show the OSD.
- A missing `ddcutil` leaves the internal panel working and hides the external controls.
- The service emits a snapshot on startup and after a device change.
