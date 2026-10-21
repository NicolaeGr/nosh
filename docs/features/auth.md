# Authentication

Current state: nosh has no authentication. AstalAuth is the reference. The user wants authentication in the shell. This file defines the scope so it does not turn into a login manager by accident.

## What AstalAuth is

`AstalAuth` is a thin C wrapper over PAM. It exposes one async operation:

```text
Pam.authenticate(password) -> success or error
```

It uses a PAM service named `astal-auth` by default. It does not manage sessions, users, or the greeter. It only answers "is this password valid for this user?".

PAM can send multiple messages. The simple `authenticate` call handles the common password-only case. A full implementation needs a conversation callback for username, password, OTP, and informational messages.

## Scope

Authentication is useful for:

- A lock screen that covers the session until the password is verified.
- A power menu that asks for the password before shutdown, reboot, suspend, or hibernate.
- A user switcher that verifies the target user's password before starting a session.
- A privilege prompt for a shell action, if the action cannot use polkit.

It is not:

- A greeter. That is `AstalGreet` and greetd, covered separately.
- A replacement for polkit in system services.
- A session manager.

Start with the power menu or a lock screen. Do not add both in the first pass.

## Core

Add an `AuthService` to `nosh-core`.

### Backend

Use a PAM binding crate or a small `libpam` FFI. Keep the PAM conversation in a dedicated blocking thread. PAM calls can block and may invoke the conversation multiple times. Do not run PAM on the Tokio runtime thread or the GTK thread.

PAM service try tree:

| Order | Service | Probe | Behavior |
| --- | --- | --- | --- |
| 1 | configured service, default `nosh-auth` | service file exists | Normal authentication |
| 2 | `login` | configured service is missing and the user allows the fallback | Warn once, then use `login` |
| 3 | none | always | Hide auth entry points |

Do not silently fall back to a different PAM service. A wrong service stack can grant more than intended. The fallback is opt-in in `config.toml`.

Suggested shape:

```rust
pub struct AuthRequest {
    pub service: String,      // default "nosh-auth"
    pub username: String,
    pub password: Zeroizing<String>,
}

pub enum AuthEvent {
    Success,
    Failed { message: String },
    Info { message: String },
    Prompt { kind: PromptKind, message: String },
}
```

If the simple password flow is enough, one `authenticate` call is fine. If the shell supports OTP or a username prompt, use a conversation state machine.

### PAM service

Ship a PAM service file:

```text
/etc/pam.d/nosh-auth
```

Start with the same stack as a normal login service, for example `system-auth` on Arch or `common-auth` on Debian. Document that the user must install it. On NixOS, the service must be added to `security.pam.services`.

Do not reuse the `astal-auth` service name. The rewrite should not depend on Astal being installed.

### Security

- Never log the password or the PAM response.
- Zeroize the password buffer after use. Use `zeroize` and `secrecy`.
- Do not send the password over D-Bus or a socket. Keep the authentication in-process.
- Rate limit failed attempts in the UI and the service, for example one attempt per second and a short lockout after repeated failures.
- Do not echo the password. Use a `gtk::PasswordEntry`.
- Clear the entry after a failed attempt.
- Do not store the password.
- Run one authentication at a time.

### Commands

```rust
Command::AuthAuthenticate(AuthRequest)
Command::AuthCancel
```

Return the result as an `AuthEvent` on a `oneshot` channel or as a normal event with a request ID. Do not block the UI waiting for PAM.

## UI

Start with a modal password dialog or a lock screen.

### Host UI

The first auth integration is a power menu overlay on the current monitor. It lists Shutdown, Reboot, Logout, Suspend, and Hibernate. Each action opens the password dialog, then calls the matching logind method. If the power menu is deferred, auth has no visible entry point and should not be wired yet. See `features/power-menu.md`.

### Power menu dialog

- A small layer-shell or modal window on the current monitor.
- Title: the action being confirmed.
- Username prefilled from the session user, editable if the action targets another user.
- Password entry.
- Cancel and Confirm buttons.
- Error label for a failed attempt.
- Enter confirms, Escape cancels.
- On success, send the action command. On failure, clear the password and show the error.

### Lock screen

A lock screen needs more than PAM:

- Cover every monitor with an input-blocking layer surface.
- Render a lock UI.
- Inhibit idle and prevent suspend if configured.
- Optionally blank the screen after a timeout.
- Unlock on PAM success.
- Handle monitor hot-plug while locked.
- Do not allow the shell's other windows to take focus.

This is a larger project. Do not build it in the first auth pass unless the user asks for it.

### Power menu and user switcher

The README lists a power menu and user switcher as planned. Auth fits there. The same dialog can confirm shutdown, reboot, suspend, hibernate, logout, or user switch. A user switcher also needs a session list from logind, which is a separate `logind` D-Bus service.

## Configuration

```toml
[auth]
service = "nosh-auth"
lockout_attempts = 5
lockout_seconds = 30
```

Keep the defaults conservative. Do not allow an empty password.

## Definition of done

- A password dialog verifies a user through PAM.
- A correct password returns success and the action proceeds.
- A wrong password shows an error, clears the entry, and allows another attempt.
- The password is never logged, stored, or sent over IPC.
- PAM runs off the GTK thread and off the Tokio runtime thread.
- The PAM service file and its installation are documented.
- The shell still starts and works when PAM or the service file is missing.
- A lock screen is explicitly out of scope until it gets its own spec.
