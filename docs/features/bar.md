# Bar

Current files: `src/bar/Bar.vala`, `Left.vala`, `Center.vala`, `Right.vala`, `src/bar/Bar.scss`.

## Current behavior

The bar is an `Astal.Window` with:

- Anchor top, left, and right.
- Exclusivity `EXCLUSIVE`.
- Namespace `nosh-topbar`.
- CSS classes `Bar`, `min-h-6`, `px-1`, `py-0`, `mx-1`, `text-sm`, `font-bold`.
- Background `rgba(0, 0, 0, 0.1)`.
- A `Gtk.CenterBox` with three children.

Left: a `Gtk.Box` aligned start, expanding horizontally, containing `Widgets.Workspaces`.

Center: a `Gtk.Box` containing `Widgets.Time` and `Widgets.Media`.

Right: a `Gtk.Box` aligned end, expanding horizontally, CSS class `Right`, containing, in order:

1. `SystemStats`
2. `SysTray`
3. `IdleInhibitor`
4. `Battery`
5. `Wifi`
6. `Volume`
7. `QuickSettingsButton`

The bar calls `present()` in its constructor, after `App` adds it to the application.

`App.vala` constructs `new TopBar.Bar(false)` for a stable build and `new TopBar.Bar(true)` for a dev build. The dev build adds the CSS class `secondary-instance`, which draws a one-pixel warning-colored top border.

`Bar` itself does not set a height. `min-h-6` is 24 px. `px-1` and `mx-1` are 4 px. `text-sm` is 14 px. `font-bold` is 700.

The current bar does not set a monitor. Hyprland chooses the output for the layer surface. The rewrite changes this: every enabled monitor gets its own bar.

## Core

The bar is UI. It owns no system state. It consumes these core services:

- `WorkspaceService` for workspaces.
- `MonitorService` for the monitor layout and per-monitor bar set.
- `AudioService` for volume.
- `BatteryService`.
- `NetworkService`.
- `TrayService`.
- `MprisService`.
- `IdleInhibitService`.

The core must emit a snapshot containing the initial state for each service so the bar can render without waiting for a change event. If a service is unavailable, the bar hides or disables the matching widget. It must not wait for the service to start.

## UI

### Window

Create a `gtk::Window`, disable decorations and resizing, call `gtk4_layer_shell::init_for_window`, then:

```rust
gtk4_layer_shell::set_anchor(&window, Edge::Top, true);
gtk4_layer_shell::set_anchor(&window, Edge::Left, true);
gtk4_layer_shell::set_anchor(&window, Edge::Right, true);
gtk4_layer_shell::auto_exclusive_zone_enable(&window);
gtk4_layer_shell::set_namespace(&window, "nosh-topbar");
gtk4_layer_shell::set_keyboard_mode(&window, KeyboardMode::None);
```

Use the auto exclusive zone. Do not hardcode the height. The content height is the reserved zone.

Set `width_request = 1`, `height_request = 1` before mapping, then let the `CenterBox` expand horizontally.

### Layout

Use `gtk::CenterBox`:

- `set_start_widget` gets the left box.
- `set_center_widget` gets the center box.
- `set_end_widget` gets the right box.

Left and right boxes set `hexpand = true` and the matching `halign` (`Start`, `End`). The center box keeps its natural width.

The right box uses `spacing = 0` like the current `Gtk.Box` default. Widgets carry their own spacing.

Apply the same CSS classes with `add_css_class`:

```text
Bar min-h-6 px-1 py-0 mx-1 text-sm font-bold
```

In a dev build, add `secondary-instance`.

### Time widget

Current: `src/bar/widgets/Time.vala`.

- Format `%H:%M - %a %e`.
- Updated every 1000 ms.
- CSS class `Time`.
- Removes the timer when the widget is destroyed.

Rust: a `gtk::Label` in a `gtk::Box` with class `Time`. Use `glib::timeout_add_seconds_local(1, ...)`. Format with `glib::DateTime::now_local()` and `format("%H:%M - %a %e")`, or `chrono` if you prefer. Remove the source when the widget is unparented, or attach it to the widget with a weak reference.

Make the format a config value in `config.toml` with the current string as the default. That is a small addition, but it avoids recompiling for a common preference. If exact parity is required, keep the constant.

### Quick settings button

Current: `src/bar/widgets/QuickSettingsButton.vala`.

- CSS class `square-icon`.
- Icon `preferences-other-symbolic`.
- Clicking toggles `AppState.quick_settings_open`.

Rust: a `gtk::Button` with a `gtk::Image`. On click, flip the UI-owned `quick_settings_open` boolean and show or hide the quick settings window. Do not send a core command.

The `square-icon` class is defined in `.square-icon, .menu-icon button` in `src/style/_blocks.scss`: 32x32 minimum, transparent background, hover `rgba(255, 255, 255, 0.1)`, image 30x30.

### Widget order

Keep the order. It is part of the visual identity and affects how the bar is read left to right. If a widget is hidden because its hardware is absent, the remaining widgets keep their order.

### Multi-monitor

Create one `BarWindow` per enabled `gdk::Monitor`. Each bar:

- Calls `gtk4_layer_shell::set_monitor` with its monitor before presenting.
- Uses the same namespace `nosh-topbar` so compositor rules apply to all bars.
- Shows only the workspaces for its monitor. Pass the monitor connector to the workspaces widget and filter by `WorkspaceModel.monitor`.
- Rebuilds when `gdk::Display::monitors_changed` fires or the core emits a monitor layout change.

When a monitor is removed, destroy its bar. Move any window that was on that monitor to the current monitor. Do not leave a layer surface on a removed output.

The bar section layout, CSS, and widget order are identical on every monitor. Do not hide the bar on secondary monitors.

## CSS

Keep these selectors from `src/bar/Bar.scss`:

```css
window.Bar { border: none; box-shadow: none; background-color: rgba(0,0,0,0.1); }
window.Bar.secondary-instance { border-top: 1px solid alpha(@brown_2, 0.8); }
```

The utility classes are generated by `src/style/common/`. Do not inline their values into the Rust code. Keep them in the stylesheet.

## Definition of done

- Every enabled monitor has a bar at the top of that monitor with namespace `nosh-topbar` and a reserved zone.
- Left, center, and right sections match the current order and alignment.
- The time label updates once per second and uses `%H:%M - %a %e`.
- The quick settings button toggles the quick settings window.
- A dev build shows the `secondary-instance` border.
- The bar starts and remains visible when every optional service is unavailable.
- The bar has no direct D-Bus, Hyprland, sysfs, or process calls.
- Adding and removing a monitor adds and removes its bar without stale surfaces.
- Each bar shows only its own monitor's workspaces.
