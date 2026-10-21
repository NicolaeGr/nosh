# Backend try trees

Some features have several possible implementations. Monitor layout is the clearest example: Hyprland has its own runtime API, another compositor may have its own, wlroots compositors may expose `zwlr_output_manager_v1`, and any Wayland session can at least be read through GDK and `wl_output`.

Do not scatter `if compositor == ...` checks through feature code. Each implementation is a backend behind a trait. A resolver tries the backends from most specific to most generic and selects the first one that satisfies the required capability set.

Call this a **try tree**. It is a tree because a feature can have sub-features, and each sub-feature can resolve separately. Monitor read and monitor write are separate capabilities. Workspace read, workspace activate, and workspace move can resolve separately. Brightness resolves per device.

## Rules

1. Specific backends come before generic ones.
2. A backend is selected only if its probe succeeds and it supports the capability the caller needs.
3. Probing has no side effects. Do not change monitor layout, start scans, or spawn long-lived processes during a probe.
4. Selection is cached. Do not probe on every call.
5. A runtime failure marks the backend degraded, emits an event, and moves to the next candidate.
6. A degraded specific backend is re-probed with backoff. When it recovers, the resolver may switch back after a short stability window, for example 5 seconds.
7. Capabilities are resolved independently. A feature can use one backend for read and another for write if that is the best available pair.
8. Log the candidate list, probe results, and selected backend at startup at debug level. Emit the selected backend in the state snapshot so a bug report can show it.
9. A user config override is allowed. Example: `workspaces.backend = "auto" | "hyprland" | "ext"`. Auto is the try tree.
10. A backend that cannot support a required capability is not an error if a later candidate can. The feature reports a reduced capability only when no candidate supports it.

## Resolver shape

```rust
pub struct Candidate<B> {
    pub name: &'static str,
    pub make: Box<dyn Fn() -> Result<B, BackendError> + Send + Sync>,
}

pub struct Resolved<B> {
    pub backend: B,
    pub name: &'static str,
    pub capabilities: Capabilities,
}

pub async fn resolve<B: Backend>(
    feature: &'static str,
    candidates: Vec<Candidate<B>>,
    required: Capabilities,
) -> Result<Resolved<B>, BackendError> {
    for candidate in candidates {
        match (candidate.make)() {
            Ok(backend) => match backend.probe().await {
                Ok(caps) if caps.contains(required) => {
                    tracing::info!(feature, backend = candidate.name, ?caps, "selected backend");
                    return Ok(Resolved { backend, name: candidate.name, capabilities: caps });
                }
                Ok(caps) => {
                    tracing::debug!(feature, backend = candidate.name, ?caps, "backend lacks required capability");
                }
                Err(error) => {
                    tracing::debug!(feature, backend = candidate.name, %error, "probe failed");
                }
            },
            Err(error) => {
                tracing::debug!(feature, backend = candidate.name, %error, "constructor failed");
            }
        }
    }

    Err(BackendError::NoBackend(feature))
}
```

The real service keeps the candidates and can re-run the resolver after a failure. Do not rebuild the entire feature when one backend degrades.

## Runtime fallback

```text
selected backend healthy
        |
        | operation fails, or probe/capability lost
        v
mark degraded -> emit BackendChanged -> resolve next candidate
        |
        +--> periodic probe of the degraded specific backend
                |
                | healthy for stability window
                v
          switch back and emit BackendChanged
```

Use a `tokio` task for re-probe. Use a backoff from 1 second to 60 seconds. Do not switch back on the first successful probe. Require a stable period so a flapping compositor or DDC bus does not oscillate the UI.

## Capability sets

Define small enums or bitflags per feature. Do not use one global capability enum.

Examples:

```rust
bitflags::bitflags! {
    pub struct MonitorCaps: u8 {
        const READ          = 1 << 0;
        const WATCH         = 1 << 1;
        const WRITE         = 1 << 2;
        const MIRROR        = 1 << 3;
        const ENABLE        = 1 << 4;
        const SET_PRIMARY   = 1 << 5;
    }

    pub struct WorkspaceCaps: u8 {
        const READ          = 1 << 0;
        const WATCH         = 1 << 1;
        const ACTIVATE      = 1 << 2;
        const MOVE          = 1 << 3;
        const URGENT        = 1 << 4;
        const SPECIAL       = 1 << 5;
    }
}
```

## Monitors

Try order for reads and events:

1. `HyprlandMonitorBackend`. Detect `HYPRLAND_INSTANCE_SIGNATURE`, a working `hyprctl -j version`, and `hyprctl -j monitors`.
2. `CompositorMonitorBackend`. A compositor-specific adapter for the running compositor, if one exists. There is no concrete implementation beyond Hyprland yet.
3. `WlrOutputManagementBackend`. Bind `zwlr_output_manager_v1`. This is wlroots-specific but more generic than a shell command.
4. `WaylandReadOnlyMonitorBackend`. GDK plus `wl_output` and `zxdg_output_manager_v1`.
5. `StaticMonitorBackend`. One monitor from GDK, no connector changes.

Try order for writes:

1. Hyprland runtime monitor API, provider and version aware.
2. A compositor-specific write adapter, if one exists.
3. `zwlr_output_manager_v1`.
4. No write backend.

The service can select Hyprland for read, watch, and write. If the Hyprland write path fails after a version change, it keeps Hyprland reads and resolves a generic write backend or disables layout presets. Do not lose the bars and notification placement because writes are unavailable.

## Workspaces

Try order:

1. `HyprlandWorkspaceBackend`. Specific. Read, watch, activate, move, urgent, special.
2. `CompositorWorkspaceBackend`. A compositor-specific adapter for the running compositor, if one exists. There is no concrete implementation beyond Hyprland yet.
3. `ExtWorkspaceBackend`. Generic `ext-workspace-v1`. Read, watch, activate, move where the handle advertises the capability. Urgent when the compositor reports `URGENT`. Special workspaces are not represented, so hide them by name.
4. `StaticWorkspaceBackend`. Empty list.

This is specific first. Hyprland is preferred even when `ext-workspace-v1` is available because the Hyprland backend has exact urgent semantics, special workspace knowledge, and the current behavior users already see. The generic backend is the fallback and the path for compositors without an adapter.

If the specific backend lacks a capability, for example a compositor adapter cannot move workspaces between monitors, the resolver can use the generic backend for that operation while keeping the specific backend for reads. Keep the two capability selections separate.

## Audio

Try order:

1. `WirePlumberNativeBackend`. Only if the rewrite adds a direct libwireplumber binding. Not planned for v1.
2. `PulseBackend` through `libpulse-binding`. Default on PipeWire and PulseAudio.
3. `WpctlBackend` through `wpctl` and `pactl subscribe`. CLI fallback.
4. `NullAudioBackend`. The shell starts, audio widgets hide.

Probe Pulse by opening a context and waiting for the server info. Probe `wpctl` by checking that the binary exists and `wpctl get-volume @DEFAULT_AUDIO_SINK@` returns a value. Do not treat a missing default sink as unavailable, because a machine can have no sink until one appears.

## Brightness

Brightness resolves per device, not one backend for everything.

Internal panel:

1. `SysfsLogindBackend`. Read sysfs, write through logind.
2. `BrightnessctlBackend`.
3. `CommandBackend`.

External display:

1. `DdcutilBackend`.
2. `DdcHiBackend`.
3. `CommandBackend`.

Master:

1. `CompositeBackend`. Fans a target value out to every available device.
2. `SingleDeviceBackend`. Used when only one device exists.

The actor queue is not a backend. It is the execution model above the backends. A slow DDC backend still goes through the queue.

## Notifications

Daemon role:

1. `OwnDaemonBackend`. Request `org.freedesktop.Notifications`.
2. `ExistingDaemonProxyBackend`. If another daemon owns the name, act as a proxy and read its notifications.
3. `NullNotificationBackend`. Log and do not display.

The UI is the same in all three cases. It receives events from the core and does not care who owns the name.

## Tray

Watcher role:

1. `OwnWatcherBackend`. Own `org.kde.StatusNotifierWatcher` and act as host.
2. `ProxyWatcherBackend`. Another process owns the watcher; proxy it and register as a host.
3. `NoTrayBackend`. The bar has no tray section.

Menu role:

1. `RustDbusMenuBackend`. The only option. No fallback to appmenu-glib-translator.

If a tray item has no menu, use `Activate`. That is not a backend fallback; it is protocol behavior.

## Auth

PAM service resolution:

1. Configured service, default `nosh-auth`.
2. `login` service, only if the configured service is missing and the user allows the fallback.
3. No auth. Hide the auth entry points.

Do not silently authenticate against a different service. Emit a warning and require the user to confirm the fallback in config.

## Testing

For every try tree:

- A test with all candidates available asserts the specific backend wins.
- A test with the specific backend probe failing asserts the next candidate wins.
- A test with a runtime failure asserts degradation, event emission, and fallback.
- A test with recovery asserts the stability window before switching back.
- A test with no candidates asserts a clean reduced-capability state, not a panic.
- A test asserts that probing has no side effects.

Use fake backends and a fake clock for the stability window. Do not require Hyprland, a compositor, DDC, or a session bus for these tests.

## Definition of done

- Every feature with more than one implementation has a named try tree in this file or its feature file.
- Feature code asks for a capability, not a compositor name.
- Each backend has a probe, a capability set, and a clear runtime failure path.
- The selected backend appears in the state snapshot and the logs.
- A runtime failure falls back without restarting the shell.
- A recovered specific backend is preferred again after the stability window.
- Tests cover selection, fallback, recovery, and the no-backend case.
