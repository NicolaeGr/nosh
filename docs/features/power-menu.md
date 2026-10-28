# Power menu

Current state: implemented. The quick settings header's session button opens `nosh-power-menu` on the same monitor. The card lists the actions logind reports, then asks for a password through `AuthService`.

## Requirements

- A power menu overlay on the current monitor.
- Actions: Lock screen, Shutdown, Reboot, Logout, Suspend, Hibernate, Cancel.
- The lock entry locks the session without a logind call. See `features/lock-screen.md`.
- A lock request while the menu is open closes the menu first.
- A confirmation step for destructive actions.
- A password prompt through `AuthService` before the action.
- Logout terminates the current session. Shutdown, reboot, suspend, and hibernate use logind.
- Escape and a background click close the menu.
- The menu never appears on a monitor that is being removed.
- The menu does not block the shell if logind or PAM is unavailable.

## Core

`power::service::PowerService` wraps logind over D-Bus. It publishes a snapshot of the actions logind accepts, then runs one action per `Command::PowerAction` after the UI has authenticated the user.

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

Logout terminates the session object at `/org/freedesktop/login1/session/auto`. logind resolves that path to the caller's session, so the service does not need to parse `XDG_SESSION_ID`.

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

`LogindBackend::read` asks `CanPowerOff`, `CanReboot`, `CanSuspend`, and `CanHibernate`. Logout is always listed. An action whose answer is `no` or `na` is insensitive in the card. A capability call that fails drops that one action and keeps the rest, so one unanswered method does not hide the menu. Do not hide all actions because one is unavailable. Only when logind answers none of the four calls is the menu marked unavailable.

### Auth

The UI asks `AuthService` for the password. The core does not store the password. After success, the UI sends `PowerAction`. The auth doc is in `features/auth.md`.

Logout asks for confirmation only. logind lets a session terminate itself, and the session may hold no PAM stack that works. A lockout on Logout would trap the user in a session they cannot leave. Every other action asks for a password whenever logind answers `challenge`.

Do not let the core accept a raw password in a `PowerAction` command. Keep auth and the action separate.

## UI

### Window

Create a layer-shell window:

- All four anchors.
- Layer `Overlay`.
- Exclusivity `Ignore`.
- Keyboard mode `Exclusive`.
- Namespace `nosh-power-menu`.
- Monitor: the monitor quick settings was opened on.
- Classes `PowerMenu`, `PowerMenuBackground`, `PowerCard`.

The background is a full-screen `gtk::Box` with a dimmed backdrop. The card is centered with a vertical list of buttons.

### Layout

```text
PowerMenuWindow
  Box PowerMenuBackground
    Box PowerCard
      Label "Session"
      Label user and host
      Stack
        Box PowerActions
          Button lock
          Button log out
          Button suspend
          Button hibernate
          Button restart
          Button power off
        Box PowerAuth
          Label action
          PasswordEntry
          Label error
          Button cancel
          Button confirm
```

Use the existing icon and surface styles where they fit. The `power_menu.css` rules cover the card background, border, radius, padding, and minimum width.

### Interaction

- Click an action: switch the card to the confirmation step, titled with the action. The card shrinks to the confirmation height instead of keeping the action list height.
- The lock entry closes the menu and opens the lock screen immediately. It has no confirmation step and never depends on the logind capabilities.
- The confirmation asks for a password only when logind answers `challenge` for that action. On a normal local session `CanPowerOff`, `CanReboot`, `CanSuspend`, and `CanHibernate` answer `yes`, so a confirmation is enough.
- Correct password or no password required: send `PowerAction`. The window closes when logind accepts.
- Wrong password: stay in the same card, show the error, clear the entry.
- Cancel or Escape: return to the action list. Escape on the list closes the menu.
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

Add `crates/nosh-ui/resources/power_menu.css`. The file defines the rules the UI uses. Keep the button styling consistent with the quick settings card. Do not introduce a new visual language.

## Definition of done

- The power menu opens on the current monitor.
- The card is centered on the monitor.
- The card height follows the visible step, so the confirmation does not leave empty space below it.
- Shutdown, reboot, logout, suspend, and hibernate are available when logind supports them.
- An action that logind answers with `challenge` asks for a password. An action it answers with `yes` asks only for confirmation.
- A destructive action asks for confirmation and a password.
- A correct password sends the logind action.
- A wrong password shows an error and allows another attempt.
- Escape and a background click close the menu.
- A missing logind or PAM leaves the shell running and shows a clear error.
- The menu closes if its monitor disappears.
- Auth and power actions stay separate in the core.
- The menu uses the existing button and card styling.
