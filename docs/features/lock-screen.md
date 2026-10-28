# Lock screen

Current state: implemented. `nosh lock` locks the session through
`ext-session-lock-v1`. The same lock opens from the power menu and from a
`loginctl lock-session` request.

## Triggers

- `nosh lock`, which is forwarded to the running instance.
- The "Lock screen" entry at the top of the power menu.
- A `Lock` signal on the caller's logind session, which is what
  `loginctl lock-session` and most `hypridle` setups send.

The screen does not lock itself on idle. Idle policy stays with the compositor
or `hypridle`. Point its lock timeout at `nosh lock`.

## Core

`nosh-core/src/session_lock/` watches logind. It does not hold the lock and it
does not know what is on screen.

| File         | Role                                                      |
| ------------ | --------------------------------------------------------- |
| `backend.rs` | `SessionLockBackend` trait with `probe` and `watch`       |
| `logind.rs`  | Resolves the session with `GetSessionByPID`, then watches |
| `marker.rs`  | Records a wanted lock so a restart can reclaim it         |
| `service.rs` | Sends `Event::SessionLock(SessionLockEvent)` to the UI    |

The session object path comes from
`org.freedesktop.login1.Manager.GetSessionByPID`. The `/session/auto` path is
a method alias and does not carry the session's signals. `Lock` and `Unlock`
are read from `org.freedesktop.login1.Session`.

A missing logind sends `SessionLockEvent::Unavailable` and leaves the shell
running. The UI can still lock itself.

The core does not act on `Unlock`. Only a successful PAM check unlocks the
screen, so `loginctl unlock-session` cannot bypass it.

## Crash recovery

`ext-session-lock-v1` keeps the session locked when the client disappears, but
it does not tell a restarted client that a lock is still active. Hyprland also
leaves logind's `LockedHint` alone, so the shell cannot ask logind either.

nosh writes `$XDG_RUNTIME_DIR/nosh/session-lock` before it asks for the lock and
removes it on `unlocked` or `failed`. A start that finds the marker attempts the
lock first, before the bar is built. The runtime directory is cleared at logout,
so a marker never outlives the session it describes.

Hyprland only gives the lock to a new client when
`misc:allow_session_lock_restore` is on. Without it a crash while locked leaves
the session locked with no way back except a reboot from a TTY.

```text
misc {
    allow_session_lock_restore = true
}
```

Run nosh under a user unit with `Restart=always` so the process comes back on
its own. A crash between the marker write and the lock request locks on the next
start, which is the safe direction.

## UI

`crates/nosh-ui/src/lock_screen/mod.rs` uses the `gtk4-session-lock` crate over
the `ext-session-lock-v1` protocol. The compositor holds every input device
while the lock is active, so compositor keybinds and other clients cannot
interact with the session.

### Windows

- One `gtk::ApplicationWindow` per monitor, assigned with
  `Instance::assign_window_to_monitor`.
- A monitor plugged in while locked gets a new window from the `monitor`
  signal.
- The library destroys the windows when the session unlocks or a monitor goes
  away. The screen drops the matching saved widgets on its next clock tick.
- The shell must not create new toplevels while locked. The bar sync skips
  monitor changes while locked and runs again after the unlock. Menus and the
  power menu ignore open requests while locked, and a polkit prompt that
  arrives while locked is cancelled. Notification cards and the change
  indicator stay hidden while locked: the notification window dismisses the
  cards on screen, drops new popups, and keeps the notification in the history
  and the core model, and the indicator ignores a change until the unlock.

### Layout

```text
LockScreen window
  Box LockBackground
    Box (expanding spacer)
    Label LockClock
    Label LockDate
    Box LockCard
      Box LockIdentity
        avatar
        Box
          Label LockName
          Label LockHost
      PasswordEntry LockPassword
      Label LockError
      Button LockPrimary
    Box (expanding spacer)
```

The clock and the card are centered as one group. Clicking empty space focuses
the password entry. Enter and the button both submit.

### Authentication

- One request at a time. The service rejects a second submit while one is in
  flight.
- `Command::AuthAuthenticate` uses the `nosh-auth` PAM service. See
  `features/auth.md`.
- A wrong password clears every entry, shows the same message on every
  monitor, and leaves the screen locked.
- A request that takes longer than 30 seconds gives up and re-enables the
  entry. A late result is ignored.
- The service's failed-attempt lockout is shown as the error message.
- Success calls `Instance::unlock()`. The `unlocked` signal clears the state
  and restores the bar on monitors that appeared while locked.
- The password is never stored or logged. Only the request id crosses the
  bridge with the result.

### Request ids

The power menu and the lock screen share `auth_request::next_request_id()`.
A late reply from one window can never match a request from the other.

## CSS

`crates/nosh-ui/resources/lock_screen.css` holds the lock rules. The window
uses the 12px root font and the theme colours from `style.css`.

## Integration

Hyprland keybind:

```text
bind = $mainMod, L, exec, nosh lock
```

`hypridle` listener:

```text
listener {
    timeout = 300
    on-timeout = nosh lock
}
```

The `lock_cmd` in the `general` block should also call `nosh lock`, not
`hyprlock`. The `before_sleep_cmd = loginctl lock-session` line still works,
because logind's `Lock` signal reaches the running shell.

Do not run `hyprlock` at the same time. Only one client can hold
`ext-session-lock-v1`, so the second one fails with a logged warning.

## Not covered

- Auto-lock policy. That belongs to `hypridle` or the compositor.
- Suspending before lock or inhibiting suspend while locked.
- Blanking the screen after a timeout while locked.
- A blurred screenshot of the desktop behind the card.
- Unlocking through logind. Only PAM unlocks.

## Definition of done

- `nosh lock` covers every monitor and blocks input.
- `loginctl lock-session` opens the same screen.
- A wrong password keeps the screen locked and shows the error.
- A correct password unlocks and restores the bar.
- The screen stays locked when logind sends `Unlock`.
- A monitor added while locked gets a lock window.
- A crash while locked re-locks on the next start, and a refused lock is logged
  as an error.
- The bar and the polkit prompt stay out of the way while locked.
- The password is never logged, stored, or sent over D-Bus.
