# Power profiles

Current state: not used. `src/meson.build` lists `astal-power-profiles-0.1`, but no source imports it. The rewrite adds it now because the user wants a power mode control in quick settings.

## Astal library

`AstalPowerProfiles` is 224 Vala lines over the D-Bus service `org.freedesktop.UPower.PowerProfiles` at `/org/freedesktop/UPower/PowerProfiles`.

It exposes:

- `active_profile` (`power-saver`, `balanced`, `performance`).
- `icon_name` as `power-profile-{active_profile}-symbolic`.
- `actions`.
- `active_profile_holds`.
- `performance_degraded`.
- `profiles` with `profile`, `cpu_driver`, `platform_driver`, `driver`.
- `version`.
- `hold_profile(profile, reason, application_id) -> cookie`.
- `release_profile(cookie)`.
- `profile_released(cookie)` signal.

The daemon may be absent. Treat it as an unavailable service, not an error.

## Core

Add a `PowerProfilesService` to `nosh-core` in the first rewrite.

### D-Bus

System bus, service `org.freedesktop.UPower.PowerProfiles`, path `/org/freedesktop/UPower/PowerProfiles`, interface `org.freedesktop.UPower.PowerProfiles`.

Read `ActiveProfile`, `Profiles`, `Actions`, `PerformanceDegraded`, `Version`. Subscribe to `PropertiesChanged` and `ProfileReleased`.

If the service is absent, emit `Unavailable`. Retry with backoff. Do not fail startup.

### Model

```rust
pub struct PowerProfilesModel {
    pub available: bool,
    pub active: PowerProfile,
    pub profiles: Vec<PowerProfileInfo>,
    pub degraded: Option<String>,
}

pub enum PowerProfile {
    PowerSaver,
    Balanced,
    Performance,
    Unknown(String),
}

pub enum PowerProfilesEvent {
    Snapshot(PowerProfilesModel),
    Changed(PowerProfilesModel),
    Unavailable,
}
```

Map the profile strings exactly. Keep `Unknown(String)` so an unknown daemon value does not break the UI.

### Command

```rust
Command::PowerProfileSet(PowerProfile)
Command::PowerProfileCycle
```

`PowerProfileCycle` chooses the next available profile in `power-saver -> balanced -> performance` order and calls `PowerProfileSet`.

Set `ActiveProfile` and wait for `PropertiesChanged`. Do not update the local state optimistically unless the UI shows a pending state. Log a failed set once and leave the previous state.

Holds are not needed for the quick settings control. Implement `hold_profile` only for a feature that needs to keep a profile active while an operation runs, for example a game mode or a compile task.

## UI

Place a fourth control in the quick settings toggle row. The row currently has DND, No Sleep, and VPN. A four-button row still fits the 260 px card if each button keeps the current `large-button` max width of 80 px. Use one of these designs:

### Recommended: cycling large button

A `LargeButton` with:

- Icon `power-profile-{profile}-symbolic`.
- Status `Power Saver`, `Balanced`, or `Performance`.
- Click cycles through the available profiles in the order `power-saver -> balanced -> performance -> power-saver`.
- Tooltip shows the current profile and the `performance_degraded` reason when it is non-empty.
- Disabled with a tooltip when the daemon is unavailable.
- Active class only when the profile is not balanced? No. The control shows the current profile, not an on/off state. Do not use the `active` class.

The status text is longer than 10 characters. The current `LargeButton` ticker scrolls. For the power control, prefer ellipsis plus a tooltip. Keep the ticker only if the design requires it.

### Alternative: three-segment control

A horizontal row of three small buttons inside one `LargeButton` area. This is clearer but takes more horizontal space. Do not use it in the 260 px card without a layout test.

### Toggle row

Keep the order:

1. DND
2. No Sleep
3. Power profile
4. VPN

VPN stays disabled until a VPN service exists. If four buttons crowd the card, move the power control to its own row under the sliders. Do not shrink the touch targets.

## Definition of done

- The quick settings card shows the active power profile.
- Clicking the control changes the profile through D-Bus.
- The icon and label update from `PropertiesChanged`.
- A missing daemon disables the control without breaking the rest of quick settings.
- A `performance_degraded` value appears in the tooltip.
- The control does not claim a profile change before the daemon confirms it.
- The service is part of the default core startup, not an optional future module.
