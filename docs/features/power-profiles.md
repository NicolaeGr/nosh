# Power profiles

Current state: implemented in `crates/nosh-core/src/power_profiles` and
`crates/nosh-ui/src/quick_settings/toggles.rs`. The original Vala shell listed
`astal-power-profiles-0.1` but never imported it.

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
    pub active: PowerProfile,
    pub profiles: Vec<PowerProfile>,
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

The control is one item in the quick settings toggle grid, after KDE Connect.

- The icon is `power-profile-{profile}-symbolic` and the status line shows the
  active profile name.
- Clicking the left side sends `PowerProfileCycle`. The daemon list order sets
  the cycle order, and the last profile wraps to the first.
- The disclosure arrow opens a list of the available profiles. Selecting one
  sends `PowerProfileSet` and closes the list.
- Balanced is the neutral state, so the toggle only gets the `on` class for
  power-saver and performance.
- The toggle hides entirely while the daemon is missing. It has no disabled
  state, so a missing soft dependency does not leave a dead control in the
  menu.
- A non-empty `PerformanceDegraded` value appears in the toggle tooltip.
- Opening the profile list closes the slider device lists, and closing quick
  settings closes every dropdown.

The list reuses the slider dropdown surface. One profile row shows a check mark
for the active profile.

## Definition of done

- The quick settings card shows the active power profile when the daemon is
  present.
- Clicking the control cycles the profile through D-Bus, and the disclosure
  list sets a profile directly.
- The icon and label update from `PropertiesChanged`.
- A missing daemon hides the control without breaking the rest of quick
  settings, and the service keeps retrying.
- A `performance_degraded` value appears in the tooltip.
- The control does not claim a profile change before the daemon confirms it.
- The service is part of the default core startup, not an optional future module.
