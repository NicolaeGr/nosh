# Authentication

Current state: implemented. `nosh-core/src/auth` loads libpam at runtime, runs the conversation on a blocking thread, and answers one request at a time. The power menu is the only caller.

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

The implementation is `auth::pam` plus `auth::backend::PamBackend`. The PAM library is opened with `dlopen` on first use, so the shell still builds and starts on a system without PAM. The lookup tries `libpam.so.0`, the directories in `LD_LIBRARY_PATH`, `/run/current-system/sw/lib/libpam.so.0`, and then `*-linux-pam-*` entries in `/nix/store`.

PAM blocks during the conversation, so the service runs each check through `tokio::task::spawn_blocking`. The password is a `Secret`, a byte buffer that is zeroed when the last copy is dropped. It is never logged or sent over D-Bus.

PAM service try tree:

| Order | Service                                 | Probe                                                    | Behavior                            |
| ----- | --------------------------------------- | -------------------------------------------------------- | ----------------------------------- |
| 1     | configured service, default `nosh-auth` | `/etc/pam.d/<service>` exists                            | Normal authentication               |
| 2     | `login`                                 | configured service is missing and `allow_fallback` is on | Warn once, then use `login`         |
| 3     | none                                    | always                                                   | Reject every request with a message |

The fallback is on by default so the power menu works on a machine that has not installed `nosh-auth` yet. Set `allow_fallback = false` to keep the shell on the configured stack only. A wrong service stack can grant more than intended, so a distribution should install `nosh-auth` and turn the fallback off.

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

Use the same stack as a normal login service. On Arch the file contains `auth include system-auth`. On Debian it contains `auth include common-auth`. On NixOS the `nix/modules.nix` NixOS module adds `security.pam.services.nosh-auth = { }`, which generates the default stack.

With the file in place set `allow_fallback = false`. Do not reuse the `astal-auth` service name. The rewrite should not depend on Astal being installed.

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
Command::AuthAuthenticate { request_id, username, password }
```

`password` is a `Secret`. The service replies with `Event::Auth(AuthEvent::Finished(AuthResult))`, which carries the same `request_id` and an optional error message. The UI ignores a reply whose id does not match the open dialog.

## UI

The power menu is the only entry point.

### Host UI

The power menu overlay opens on the monitor where quick settings was or on the active monitor for `nosh session`. It lists Log out, Suspend, Hibernate, Restart, and Power off. Every action asks for confirmation. The password step appears only when logind answers `challenge` for that action. See `features/power-menu.md`.

### Password step

- The card title becomes the action being confirmed.
- Username comes from the session user and is not editable yet.
- `gtk::PasswordEntry` with a peek icon.
- Cancel returns to the action list. Confirm sends the request.
- Error label for a failed attempt. The password is cleared on every failure.
- Enter confirms, Escape goes back to the action list.
- On success, send `PowerAction`. On failure, clear the password and show the error.

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

`AuthServiceConfig` holds `service`, `allow_fallback`, `lockout_attempts`, and `lockout`. The defaults are `nosh-auth`, fallback on, 5 attempts, and 30 seconds. Keep the defaults conservative. An empty password is rejected in the UI.

## Definition of done

- A password entry verifies a user through PAM.
- A correct password returns success and the action proceeds.
- A wrong password shows an error, clears the entry, and allows another attempt.
- Repeated failures are rate limited.
- The password is never logged, stored, or sent over IPC.
- PAM runs off the GTK thread and off the Tokio runtime thread.
- The PAM service file and its installation are documented.
- The shell still starts and works when PAM or the service file is missing.
- A lock screen is explicitly out of scope until it gets its own spec.
