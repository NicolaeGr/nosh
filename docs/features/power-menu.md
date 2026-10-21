# Power menu

Current state: none. The README lists a power menu and user switcher as planned. Authentication has no host UI until this exists. This file defines the first auth integration.

## Requirements

- A power menu overlay on the current monitor.
- Actions: Shutdown, Reboot, Logout, Suspend, Hibernate, Cancel.
- A confirmation step for destructive actions.
- A password prompt through `AuthService` before the action.
- Logout terminates the current session. Shutdown, reboot, suspend, and hibernate use logind.
- Escape and a background click close the menu.
- The menu never appears on a monitor that is being removed.
- The menu does not block the shell if logind or PAM is unavailable.

## Core

Add a `PowerService` to `nosh-core`. It wraps logind over D-Bus and sends the action after auth succeeds.

### D-Bus

System bus, service `org.freedesktop.login1`, path `/org/freedesktop/login1`, interface `org.freedesktop.login1.Manager`.

```text
method PowerOff(interactive: b)
method Reboot(interactive: b)
method Suspend(interactive: b)
method Hibernate(interactive: b)
method HybridSleep(interactive: b)
method LockSessions()
method TerminateSession(session_id: s)
```

Logout needs the current session. Read `XDG_SESSION_ID` from the environment, or find the session that belongs to the current process through logind. If the id is missing, fall back to `Terminate()` on the session object at `/org/freedesktop/login1/session/auto`.

### Model

```rust
pub enum PowerAction {
    Shutdown,
    Reboot,
    Logout,
    Suspend,
    Hibernate,
}

pub enum PowerEvent {
    Available(bool),
    ActionStarted(PowerAction),
    ActionFailed { action: PowerAction, message: String },
    ActionCompleted(PowerAction),
}
```

### Commands

```rust
Command::PowerAction(PowerAction)
```

The service:

1. Checks that the action is available from logind.
2. Sends `ActionStarted`.
3. Calls the logind method with `interactive = true`.
4. Emits `ActionCompleted` or `ActionFailed`.

Do not retry a failed power action in a loop. `AccessDenied` and `InteractiveAuthenticationRequired` are normal outcomes and should be shown to the user.

### Availability

Read logind capabilities or call the method and handle the error. Do not hide all actions because one is unavailable. For example, a desktop may support suspend but not hibernate.

### Auth

The UI asks `AuthService` for the password. The core does not store the password. After success, the UI sends `PowerAction`. The auth doc is in `features/auth.md`.

Do not let the core accept a raw password in a `PowerAction` command. Keep auth and the action separate.

## UI

### Window

Create a layer-shell window:

- All four anchors.
- Layer `Overlay`.
- Exclusivity `Ignore`.
- Keyboard mode `Exclusive`.
- Namespace `nosh-power-menu`.
- Monitor: current monitor.
- Classes `PowerMenu`, `background`, `power-card`.

The background is a full-screen `gtk::Box` with a dimmed backdrop. The card is centered with a vertical list of buttons.

### Layout

```text
PowerMenuWindow
  Box background
    Box power-card
      Label "Power"
      Button "Shutdown"
      Button "Reboot"
      Button "Logout"
      Button "Suspend"
      Button "Hibernate"
      Button "Cancel"
```

Use the existing `square-icon` and button styles where they fit. Add `.power-card` rules for the card background, border, radius, padding, and minimum width.

### Interaction

- Click an action: open the auth dialog, titled with the action.
- Correct password: send `PowerAction`.
- Wrong password: stay in the dialog, show the error, clear the entry.
- Cancel or Escape: close the auth dialog, then the power menu.
- Background click: close the power menu.
- On successful shutdown, reboot, or logout, the process may be terminated by the session. Do not try to keep the UI alive.

### Keyboard

- Up and Down move between actions.
- Enter activates the focused action.
- Escape closes the auth dialog first, then the power menu.
- Tab moves focus in the standard order.
- Do not let the bar or other windows take focus while the menu is open.

### Monitor changes

If the current monitor disappears while the menu is open, close the menu. Do not leave a layer surface on a removed output.

## CSS

Add:

```css
.PowerMenu { background-color: transparent; }
.PowerMenu .background { background-color: rgba(0, 0, 0, 0.35); }
.PowerMenu .power-card {
  background-color: @window_bg_color;
  border: 1px solid alpha(@window_fg_color, 0.12);
  border-radius: 12px;
  padding: 16px;
  min-width: 260px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
}
```

Keep the button styling consistent with the quick settings card. Do not introduce a new visual language.

## Definition of done

- The power menu opens on the current monitor.
- Shutdown, reboot, logout, suspend, and hibernate are available when logind supports them.
- A destructive action asks for confirmation and a password.
- A correct password sends the logind action.
- A wrong password shows an error and allows another attempt.
- Escape and a background click close the menu.
- A missing logind or PAM leaves the shell running and shows a clear error.
- The menu closes if its monitor disappears.
- Auth and power actions stay separate in the core.
- The menu uses the existing button and card styling.
