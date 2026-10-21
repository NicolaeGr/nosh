# Idle inhibitor

Current files: `src/utils/Zwp/IdleInhibitorManager.vala`, `src/utils/Zwp/idle-inhibit.vapi`, `src/bar/widgets/IdleInhibitor.vala`, `src/AppState.vala`.
Reference for the other protocol: `astal-source/lib/idle-notify/src/notifier.vala`, `notification.vala`.

## Current behavior

`IdleInhibitorManager` is a singleton. It connects a second Wayland connection:

- Reads `WAYLAND_DISPLAY`.
- Calls `new Wl.Display.connect(wayland_display_name)`.
- Gets the registry.
- Binds `wl_compositor` and `zwp_idle_inhibit_manager_v1`.
- Creates a `wl_surface`.
- On enable, creates `zwp_idle_inhibitor_v1` for the surface, commits, and roundtrips.
- On disable, drops the inhibitor, commits, and roundtrips.
- Reports availability only if the compositor, surface, idle inhibit manager, and setup all succeed.

The surface is a dummy surface. It has no buffer and no visible content. The inhibitor is tied to the surface's lifetime.

`AppState` exposes `idle_inhibitor_active` and binds it bidirectionally to the manager's `active` property. It also exposes `is_idle_inhibitor_available()`.

`TopBar.Widgets.IdleInhibitor` is a `Gtk.Button` with class `square-icon`, an image child, and a tooltip.

- If unavailable: insensitive, icon `dialog-error`, tooltip `Error: Idle inhibitor not available`.
- Active: icon `caffeine-cup-full`, tooltip `Active`.
- Inactive: icon `caffeine-cup-empty`, tooltip `Inactive`.
- Click toggles `AppState.idle_inhibitor_active`.

The state is not persisted. A restart starts inactive.

## Difference from AstalIdleNotify

The current nosh code is an idle inhibitor, not an idle indicator. The naming is easy to confuse.

| Topic | nosh direct code | Astal IdleNotify |
| --- | --- | --- |
| Protocol | `zwp_idle_inhibit_manager_v1` | `ext-idle-notify-v1` |
| Purpose | Prevent the compositor from going idle | Report that a seat has been idle for a timeout |
| Object | A `zwp_idle_inhibitor_v1` attached to a `wl_surface` | A `Notification` object with `idled` and `resumed` signals |
| Direction | The shell tells the compositor to stay awake | The compositor tells the shell that the user is idle |
| State | Active or inactive inhibitor | Idle or resumed notification, with a timeout |
| Connection | Opens a second `Wl.Display` from `WAYLAND_DISPLAY` | Uses the shared `AstalWl.Registry` |
| Reconnect | None | Registry tracks global removal |
| Size | About 149 Vala lines plus a small vapi | About 130 Vala lines plus protocol bindings |

The two features are complementary. Use the inhibitor for the bar button and the No Sleep toggle. Add idle detection only if the shell gains auto-lock, screen blanking, or a hide-when-idle bar. Do not try to replace one with the other.

The direct implementation is small enough to keep. Its problems are the second connection, the lack of reconnect, and the per-toggle roundtrip. The Rust core fixes those while keeping the same protocol. If a future feature needs both idle detection and inhibition, share one Wayland connection and one registry.

## Core

Add an `IdleInhibitService` to `nosh-core`. It uses `wayland-client`, not GTK.

### Connection

Use `wayland_client::Connection::connect_to_env()`. Do not reuse GDK's connection; the core has no GDK. A second connection is valid and keeps the inhibitor independent of the UI.

Bind:

- `wl_compositor`.
- `zwp_idle_inhibit_manager_v1` from `wayland-protocols` unstable protocols.
- Optionally `wl_seat` if a future feature needs input idle detection. Not needed now.

Create one `wl_surface` at startup and keep it alive for the process. Commit it once. The surface does not need a buffer to serve as an inhibitor target.

### Enable and disable

- Enable: if no inhibitor exists, call `create_inhibitor` for the surface, commit, and flush. The inhibitor keeps the seat awake while the surface exists.
- Disable: destroy the inhibitor, commit, and flush.
- Do not roundtrip on every toggle. Flush and let the event queue drive. A roundtrip is only needed to confirm the global is bound at startup.
- Handle a compositor restart: the Wayland connection dies. Reconnect, re-bind globals, recreate the surface, and reapply the active state.

### Availability

```rust
pub enum IdleInhibitEvent {
    Snapshot { available: bool, active: bool },
    Changed { active: bool },
    Unavailable,
}
```

`available` is true only when `zwp_idle_inhibit_manager_v1` is present and the surface was created. If the protocol is missing, the service stays alive, reports unavailable, and retries on a new registry event.

### Commands

```rust
Command::IdleInhibitSet(bool)
Command::IdleInhibitToggle
```

Clamp to the current availability. If unavailable, return an error event and leave the state false. Do not claim active when the protocol is missing.

### Persistence

Do not persist the inhibitor across restarts by default. A shell restart should not silently keep the screen awake. If persistence is wanted, add `idle_inhibit = true` to `config.toml` and restore it, with a clear log.

## UI

`IdleInhibitor` is a `gtk::Button` with class `square-icon`, a centered `gtk::Image`, and a tooltip.

- Unavailable: `set_sensitive(false)`, icon `dialog-error`, tooltip `Error: Idle inhibitor not available`.
- Active: icon `caffeine-cup-full`, tooltip `Active`.
- Inactive: icon `caffeine-cup-empty`, tooltip `Inactive`.
- Click sends `Command::IdleInhibitToggle`.
- The button state follows the core event, not the click. If the command fails, the icon stays unchanged.

Do not create a second Wayland connection in the UI.

## CSS

Keep `.square-icon`. No new class is needed.

## Defects to fix

- The UI no longer owns the Wayland connection or the inhibitor.
- Availability is a core event, not a constructor side effect.
- A compositor restart reconnects and reapplies the state.
- The button does not lie while the command is in flight.
- The surface is committed once and reused, not recreated on every toggle.

## Definition of done

- The service is documented as an inhibitor, not an idle notifier.
- Clicking the bar button inhibits idle on a compositor that supports `zwp_idle_inhibit_manager_v1`.
- Clicking again releases the inhibitor.
- The icon and tooltip match the active state.
- A compositor without the protocol shows the error icon and a disabled button.
- A compositor restart reconnects and reapplies the state.
- The UI has no Wayland code.
- The state is not restored after a process restart unless the config asks for it.
