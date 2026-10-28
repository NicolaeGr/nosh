# Polkit agent

Current state: implemented. `nosh-core/src/polkit` registers an authentication agent with polkitd and routes the password prompt through the nosh card.

## Why the shell needs its own agent

polkit and PAM are separate systems. A successful PAM check through `nosh-auth` does not satisfy a polkit prompt. polkitd accepts an answer only from an authentication agent registered over D-Bus, through `org.freedesktop.PolicyKit1.Authority.RegisterAuthenticationAgent`.

Without a registered agent, polkitd cannot complete an interactive check and the desktop falls back to whatever agent is running, for example `polkit-gnome-authentication-agent-1`. That fallback is the standalone password window with the shield icon and the Details expander.

The shell must run at most one polkit agent per session. Disable any other agent before starting nosh.

## D-Bus surface

The agent uses two interfaces on the system bus.

| Side   | Service                       | Path                                       | Interface                                        |
| ------ | ----------------------------- | ------------------------------------------ | ------------------------------------------------ |
| Client | `org.freedesktop.PolicyKit1`  | `/org/freedesktop/PolicyKit1/Authority`    | `org.freedesktop.PolicyKit1.Authority`           |
| Server | none, the agent owns the path | `/org/nosh/PolicyKit1/AuthenticationAgent` | `org.freedesktop.PolicyKit1.AuthenticationAgent` |

Authority methods used:

```text
RegisterAuthenticationAgent(subject: (sa{sv}), locale: s, object_path: s)
UnregisterAuthenticationAgent(subject: (sa{sv}), object_path: s)
```

Agent methods exported:

```text
BeginAuthentication(action_id: s, message: s, icon_name: s, details: a{ss}, cookie: s, identities: a(sa{sv}))
CancelAuthentication(cookie: s)
```

The subject is `unix-session` with `session-id` from `XDG_SESSION_ID`. When the variable is missing the backend registers `unix-process` with the pid instead.

`BeginAuthentication` is a long-running call. polkitd reads the result when the method returns, so the agent keeps it open for the whole dialog. Returning early would make polkitd see an unauthenticated session.

The agent asks the bus for the caller's uid and drops the call unless it is 0. polkitd is the only process that may start a password prompt, and the cookie it passes reaches the privileged helper. A caller without that check could show a card for an action it does not own.

The card shows one prompt at a time. If polkitd starts a second authentication while one is open, the service cancels the older session first. Dropping its password channel stops its helper, and the agent ignores the late result.

## The helper

Only uid 0 may call `AuthenticationAgentResponse2`, so the unprivileged shell cannot report a successful password itself. polkit solves this with a helper:

- `/run/polkit/agent-helper.socket` exists on current polkit. The agent connects to the world-writable socket, sends the user name and the cookie, and polkitd runs the helper as root.
- Without the socket, the agent starts `polkit-agent-helper-1` with the user name as its argument and sends only the cookie. On distributions that set the binary setuid, it can still reach PAM.

Both paths speak the same line protocol:

```text
PAM_PROMPT_ECHO_OFF <text>
PAM_PROMPT_ECHO_ON <text>
PAM_TEXT_INFO <text>
PAM_ERROR_MSG <text>
SUCCESS
FAILURE
```

Lines are C-escaped. The agent writes one line per answer to the helper's input. The helper runs PAM with the `polkit-1` stack and calls `AuthenticationAgentResponse2` itself after a successful check, then prints `SUCCESS`.

A cancelled session drops the helper connection or kills the child. `BeginAuthentication` then returns without an authenticated session and polkitd fails the check.

## Flow

1. The service registers the agent and publishes `Event::ServiceAvailability(Service::Polkit, true)`.
2. polkitd calls `BeginAuthentication` on the agent object.
3. The interface starts a helper session and sends `AgentRequest::Begin` to the service.
4. The service stores the cookie with the password channel and sends `Event::Polkit(PolkitEvent::Prompt(...))`.
5. The UI shows the card.
6. The user submits a password. The UI sends `Command::PolkitRespond { cookie, password }`.
7. The service forwards the `Secret` to the helper session, which writes it to the helper.
8. The helper reports `SUCCESS` or `FAILURE`. The interface sends `AgentRequest::End`.
9. The service sends `PolkitEvent::Finished` or `PolkitEvent::Failed`, and the card closes.
10. The interface returns from `BeginAuthentication`, and polkitd applies the result.

A user cancel sends `Command::PolkitCancel`. polkitd can also cancel through `CancelAuthentication`. Both drop the password channel, which makes the helper session stop, and send `PolkitEvent::Cancelled`.

## Identity

Only `unix-user` identities are supported. The agent prefers the first identity of that kind and falls back to its own uid. The user name is resolved with `getpwuid_r`, so NSS sources such as LDAP and systemd-homed work the same way they do for polkit. `$USER` is never trusted, because a process can change it.

A request whose user cannot be resolved ends the authentication with an error instead of guessing a name.

## Password handling

The password is a `Secret`, it lives in the command channel and the helper write, and it is zeroed on drop. It is never logged and never sent over D-Bus.

## Configuration

No configuration is needed. The helper socket path is `/run/polkit/agent-helper.socket`. The binary fallback tries `/run/current-system/sw/lib/polkit-1/polkit-agent-helper-1`, `/usr/lib/polkit-1/polkit-agent-helper-1`, and `/usr/libexec/polkit-agent-helper-1`.

The NixOS module needs no extra option. polkitd and `/etc/pam.d/polkit-1` come from `security.polkit.enable`, which is on by default.

## Limitations

polkitd keeps agent registrations in memory. If the service restarts, the shell must restart to register again. The usual cause is a package upgrade that restarts `polkit.service`.

## Stopping another agent

Remove or disable the agent that was showing the old window. On NixOS the common ones are `services.hyprpolkitagent`, the polkit gnome agent started from a Hyprland `exec-once` line, and any `polkit-kde-agent` autostart entry.

Check with:

```sh
ps -eo pid,args | grep -i polkit
```

Only `polkitd` should remain once nosh owns the session.

## Definition of done

- The shell registers one agent for its session.
- A polkit check for the session shows the nosh card, not a separate application window.
- A correct password completes the check and the original action proceeds.
- A wrong password shows an error, disables the entry, and lets the user close the card.
- Cancel, Escape, and a polkit cancel all close the card and stop the helper.
- The password is never logged, stored, or sent over D-Bus.
- A missing polkitd or a second registered agent leaves the shell running.
- The agent unregisters when the core stops.
