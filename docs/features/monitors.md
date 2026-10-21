# Monitors and layout

Current state: nosh has no monitor module. The bar is one window. Hyprland chooses its output. Quick settings and the app launcher open on whatever monitor the compositor chooses. Notifications are anchored top-right on that same output. This feature adds per-monitor bars, placement rules, and a monitor configuration module.

## Requirements

- Every enabled monitor gets its own bar.
- Notifications appear on the farthest monitor in the configured corner direction, not on every monitor and not always on the primary.
- Quick settings and the app launcher open on the monitor that currently has focus.
- The shell reacts to monitor hot-plug, resolution changes, scale changes, and layout changes.
- Monitor configuration is readable in a generic way. Writing is compositor-specific.
- A monitor config module supports at least `extend`, `mirror`, `single`, and `panning` presets, with a target monitor argument.
- A CLI command or action can cycle a chosen monitor between modes for a keybind such as `super+p`.

## Model

```rust
pub struct MonitorId {
    pub compositor_id: i32,       // Hyprland id, or a stable index for other backends
    pub connector: String,        // e.g. "DP-1", "HDMI-A-1"
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
}

pub struct MonitorModel {
    pub id: MonitorId,
    pub enabled: bool,
    pub focused: bool,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub transform: Transform,
    pub refresh_rate: f64,
    pub mirror_of: Option<String>,
}

pub struct MonitorLayout {
    pub monitors: Vec<MonitorModel>,
    pub primary: Option<String>,
}
```

Use the connector name as the stable key. Compositor IDs can change across restarts.

## Read path

There are two sources, and they answer different questions.

### GDK

`gdk::Display::monitors()` gives the logical geometry, scale, and the monitor a surface is on. It is the source the UI uses for placing windows. Connect to `monitors_changed` and rebuild the bars.

`gdk::Monitor::connector()` is available in recent GTK 4 versions. If it is present, use it to match a GDK monitor to a compositor monitor. If it is not, match by geometry and scale, then by index.

### Compositor

For Hyprland, read `hyprctl -j monitors`:

```text
id, name, description, make, model, serial, width, height,
refreshRate, x, y, scale, transform, focused, dpmsStatus,
vrr, activelyTearing, disabled, currentFormat, availableModes,
reserved, activeWorkspace, specialWorkspace
```

Newer Hyprland versions also expose `mirrorOf` (open PR #305). Read it when present. If it is absent, detect a mirror by comparing geometry and the configured mirror target in `monitors.toml`.

Use the compositor data for names, ids, mirror state, reserved space, and enable or disable. Use GDK for layer-shell monitor objects. Match by connector name, then by geometry.

For a generic Wayland compositor, bind `wl_output` and `zxdg_output_manager_v1` from `AstalWl`-equivalent code. The registry gives the connector name, logical position, logical size, scale, transform, and refresh. This is read-only.

### Events

Listen to:

- GDK `monitors_changed`.
- Hyprland `monitoraddedv2`, `monitorremoved`, `monitoradded`, and `monitorremoved` events.
- Wayland registry global add and remove for `wl_output`.
- A refresh when the compositor config reloads.

On any event, emit `MonitorEvent::LayoutChanged(MonitorLayout)`. The UI rebuilds the bar set and repositions windows. Do not debounce longer than 100 ms; a monitor change should be visible quickly.

## Try tree

There is no general Wayland protocol for changing monitor layout across all compositors. Read is generic; write is an adapter. Use the try tree from [backends.md](../backends.md), specific first.

Read and watch order:

| Order | Backend | Probe | Capabilities |
| --- | --- | --- | --- |
| 1 | Hyprland monitor API | `HYPRLAND_INSTANCE_SIGNATURE`, `hyprctl -j version`, `hyprctl -j monitors` | read, watch, write, mirror, enable, primary |
| 2 | Other compositor adapter | compositor-specific socket or IPC | read, watch, write when available |
| 3 | `zwlr_output_manager_v1` | registry global | read, watch, write |
| 4 | Wayland read-only | GDK plus `wl_output` and `zxdg_output_manager_v1` | read, watch |
| 5 | Static | GDK | read |

Write order:

| Order | Backend | Capabilities |
| --- | --- | --- |
| 1 | Hyprland runtime monitor API | apply, mirror, enable, primary |
| 2 | Other compositor adapter | apply, mirror, enable when available |
| 3 | `zwlr_output_manager_v1` | apply, mirror, enable |
| 4 | None | read-only |

Resolve read and write separately. A shell can keep Hyprland reads and lose only the write path after a version change. The bars, notification placement, and current-monitor rules must keep working in read-only mode.

Define one trait per capability set:

```rust
pub trait MonitorBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> MonitorCaps;
    async fn probe(&self) -> Result<MonitorCaps, MonitorError>;
    fn apply(&self, layout: &MonitorLayout) -> Result<(), MonitorError>;
    fn cycle(&self, target: &str, mode: LayoutPreset) -> Result<(), MonitorError>;
}
```

### Hyprland

Use the Hyprland runtime interface:

- Read the current state with `hyprctl monitors -j`.
- Apply a monitor line with `hyprctl keyword monitor "<name>,<mode>,<position>,<scale>"`.
- Mirror with `hyprctl keyword monitor "<name>,<mode>,<position>,<scale>,mirror,<source>"`.
- Disable with `hyprctl keyword monitor "<name>,disable"`.
- Enable with an explicit resolution, position, and scale.
- Reload with `hyprctl reload` only when the user asks to persist a config file.

Hyprland changes its config provider. Check `hyprctl -j status` for `configProvider` and `hyprctl -j version` for the running version. Use the runtime keyword path only when the running build accepts it. If the Lua provider rejects keywords, use the compositor's Lua runtime API or write the layout to the Lua config and reload. Keep the adapter version-aware and probe capabilities at startup rather than assuming one syntax.

Do not write directly to the user's Hyprland config without a backup and an explicit action. Runtime changes are the default. Persistence is a separate command.

### Other compositor adapters

Add a specific adapter only when there is a concrete compositor to support. The adapter probes its own socket or IPC, exposes a capability set, and stays out of the default build until it has tests. Do not name or link a compositor library in the feature code. The resolver owns the order.

### wlroots

If the compositor advertises `zwlr_output_manager_v1`, use it. It can apply head state, enable or disable outputs, set mode, position, scale, transform, and adaptive sync. It does not cover every compositor, but it is the closest thing to a general write path.

### Fallback

If no write backend is available, the monitor service is read-only. The bar still appears on every monitor, notifications still place correctly, and active windows still open on the focused monitor. Only the layout presets are disabled.

## Presets

```rust
pub enum LayoutPreset {
    Extend,
    Mirror,
    Single,
    Panning,
}
```

- `Extend`: each enabled monitor gets a non-overlapping position, left to right using its logical width. Mirror is cleared.
- `Mirror`: the target monitor mirrors the source monitor, or the primary when no source is given. The target keeps its own resolution and scale when possible.
- `Single`: only the target monitor is enabled; the rest are disabled.
- `Panning`: all enabled monitors are placed in one horizontal strip with the same scale, no mirror. This is for one wide desktop that pans across displays. Treat it as a preset built from `Extend` with a fixed scale and linear positions.

`cycle(target, mode)` applies the next mode in a configured order, default `extend -> mirror -> single -> panning -> extend`. The target is a connector name or an index such as `2`.

A keybind can run:

```sh
nosh monitor-cycle 2
nosh monitor-cycle 2 mirror
nosh monitor-mode 2 panning
```

Example Hyprland binding for `super+p`:

```text
bind = SUPER, P, exec, nosh monitor-cycle 2
```

The exact CLI names are part of the rewrite spec, not the current shell. If the second monitor is not present, the command should report a short message and exit 0, not crash the shell.

## Placement rules

### Current monitor

Quick settings and the app launcher open on the current monitor.

- Prefer the compositor's focused monitor from `HyprlandService` or the generic monitor service.
- If the compositor does not expose focus, use the monitor under the pointer from `gdk::Device::surface_at_position` or `gdk::Display::monitor_at_surface` for the focused window.
- Set the layer-shell monitor before presenting. If the window is already mapped, hide it, set the monitor, then present.

The bar is not a "current monitor" window. There is one per monitor.

### Farthest monitor for notifications

Notifications have a corner setting:

```text
top-left, top-right, bottom-left, bottom-right
```

They appear on the farthest monitor in the direction of that corner. "Farthest" means the monitor whose requested corner extends furthest in that direction, not the Euclidean distance from the primary. This matches the intuition "top right means the top-right-most monitor".

Score each enabled monitor:

```rust
fn score(m: &MonitorModel, corner: Corner) -> i64 {
    let left = m.x as i64;
    let top = m.y as i64;
    let right = (m.x + m.width) as i64;
    let bottom = (m.y + m.height) as i64;

    match corner {
        Corner::TopLeft => -left * 2 - top,
        Corner::TopRight => right * 2 - top,
        Corner::BottomLeft => -left * 2 + bottom,
        Corner::BottomRight => right * 2 + bottom,
    }
}
```

Choose the highest score. Tie-break by Euclidean distance from the primary monitor center, then by connector name for determinism. If no monitor is enabled, place on the primary or the first monitor.

Allow a policy override:

```text
farthest    default, use the score
primary     always the primary monitor
current     the focused monitor
named:<x>   a specific connector
```

Put the policy and corner in our own GSettings schema so they can change at runtime:

```text
notifications position = "top-right"
notifications monitor-policy = "farthest"
notifications monitor = ""
```

These are the same keys listed in `features/notifications.md`. Do not read Astal's `io.astal.notifd` schema.

When the layout changes, recompute the target monitor and move the notification window. Do not recreate cards.

## Per-monitor bar

Create one `BarWindow` per `MonitorModel`:

- `gtk4_layer_shell::set_monitor(&window, Some(&gdk_monitor))`.
- Same namespace `nosh-topbar` for every bar so compositor rules apply to all.
- Each bar shows only its monitor's workspaces.

Workspace filtering depends on the workspace backend:

- Hyprland: `WorkspaceModel.monitor_id == monitor.id.compositor_id`.
- Ext-workspace: use the `WorkspaceGroup` that contains the `AstalWl.Output`/GDK monitor connector. The group's workspace list is the bar's list.

On monitor add, create a bar. On monitor remove, destroy the bar and move any window that was on it to the current monitor. Do not leave a layer surface on a removed output.

## Persistence

Store layout presets in `~/.config/nosh/monitors.toml`:

```toml
[layout]
apply_on_start = false
preset = "extend"

[[monitor]]
connector = "DP-1"
mode = "2560x1440@144"
position = "0x0"
scale = 1.0
mirror_of = ""
enabled = true
```

Do not apply the file at startup unless `apply_on_start = true`. Do not overwrite a Hyprland config file. Runtime apply is the default.

## Core

Add a `MonitorService` to `nosh-core` with:

- `MonitorEvent::Snapshot(MonitorLayout)`.
- `MonitorEvent::LayoutChanged(MonitorLayout)`.
- `MonitorEvent::ApplyFailed(String)`.

Commands:

```rust
Command::MonitorApply(MonitorLayout)
Command::MonitorCycle { target: String, mode: Option<LayoutPreset> }
Command::MonitorMode { target: String, mode: LayoutPreset }
Command::MonitorSetPrimary(String)
Command::MonitorSetEnabled { connector: String, enabled: bool }
```

The service reads from GDK? No. `nosh-core` has no GDK. It reads from Hyprland or Wayland. The UI has the GDK monitor objects and matches them to the core layout by connector or geometry. The UI owns the `gdk::Monitor` handles; the core owns the layout model.

If matching by connector is unavailable, the UI uses geometry and order. Document the fallback.

## Definition of done

- Every enabled monitor has a bar with the correct namespace and exclusive zone.
- Disabling or unplugging a monitor removes its bar without leaving a stale surface.
- Quick settings and the app launcher open on the focused monitor.
- Notifications appear on the farthest monitor for the configured corner.
- Changing the corner or policy moves notifications without recreating cards.
- Hyprland can cycle a target monitor through extend, mirror, single, and panning.
- A compositor without a write protocol still gets bars and placement, with layout commands disabled.
- The core exposes a monitor snapshot and a layout-changed event.
- The UI has no compositor-specific monitor write code.
- Monitor changes caused by `hyprctl` or the compositor are reflected within 100 ms.
