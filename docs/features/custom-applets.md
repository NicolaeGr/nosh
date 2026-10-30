# Custom entries, applets, and scripts

Current state: the app launcher supports custom TOML entries and two built-in search plugins. The user wants custom applets and scripts to remain a first-class feature. This file defines the extension points. If there is a separate patch with more applet types, merge its schema into this one before the rewrite.

## Current behavior

The Vala `ConfigManager` reads `~/.config/nosh/app_launcher.toml`; the rewrite reads `~/.config/nosh/apps.d/*.toml`:

```toml
[[entry]]
name = "My Custom Script"
icon = "application-x-executable"
exec = "$HOME/scripts/my-script.sh"
```

It creates an `AppEntry` with `exec_command`. Launch runs the command through Hyprland. `add_custom_entry` also accepts an in-code launch function, used by built-in plugins.

The two built-in plugins are `RunCommandPlugin` (`run:`, `kt:`, `kitty:`) and `RestartNoshPlugin`. The plugin interface has `matches`, `icon`, `name`, `priority`, and `create_entry`.

There is no generic script provider and no bar applet script.

## App launcher custom entries

Keep the `[[entry]]` table and extend it:

```toml
[[entry]]
name = "Work Firefox"
icon = "firefox"
exec = "firefox -P work"
description = "Firefox with the work profile"
keywords = ["browser", "work"]
terminal = false
working_dir = "$HOME"
env = ["MOZ_ENABLE_WAYLAND=1"]
```

Fields:

| Field         | Required | Meaning                                               |
| ------------- | -------- | ----------------------------------------------------- |
| `name`        | yes      | display name                                          |
| `exec`        | yes      | command to run                                        |
| `icon`        | no       | icon name or path, default `application-x-executable` |
| `description` | no       | searchable and shown in a tooltip                     |
| `keywords`    | no       | extra search terms                                    |
| `terminal`    | no       | wrap in the default terminal                          |
| `working_dir` | no       | child working directory                               |
| `env`         | no       | extra environment variables                           |

Entries live in `~/.config/nosh/apps.d/*.toml`. Each file may use root-level fields or the `[[entry]]` table. Keep `$HOME` expansion. Parse with a real TOML parser. A malformed file logs an error and keeps the other files' entries.

Launch uses the detached spawn helper described in `app-launcher.md`. If `terminal = true`, wrap with the configured terminal command, default `kitty -e`.

## Script search providers

A script provider lets a user add a prefix that runs a command and returns entries. This is the generic version of the `run:` and `restart` plugins.

```toml
[[provider]]
name = "Passwords"
prefix = "pw:"
icon = "dialog-password"
command = "$HOME/scripts/pass-search"
timeout_ms = 300
```

Protocol:

- The command receives the query after the prefix as one argument.
- It writes zero or more JSON objects to stdout, one per line.
- It must exit 0.
- The timeout kills the process and returns no results.

Each line:

```json
{
  "name": "GitHub",
  "icon": "web-browser",
  "description": "work account",
  "exec": "pass show github | wl-copy"
}
```

The core maps each result to an `AppEntry` with `LaunchTarget::Command`. The name, icon, description, and exec are the only required fields. `keywords` is optional.

Rules:

- Run providers off the UI thread with a timeout.
- Show at most one result set. The prefix selects exactly one provider.
- If a provider times out or exits nonzero, show no result and log once.
- Do not run a provider on every keystroke if the query has not changed.
- Do not run a provider when the prefix is absent.
- Do not let a provider shadow a normal app match unless its prefix is present.
- Do not pass the query through a shell unless the command is explicitly configured as a shell command. Prefer `Command::new(program).arg(query)`. Use `sh -c` only when the user writes `shell = true`.

Security: providers run as the user with the user's environment. They are trusted local config, not sandboxed. Do not add a network-facing provider model.

## Bar applets

A bar applet runs a command and displays its output. This is the useful part of `AstalIO.Variable` without the deprecated GObject wrapper.

```toml
[[applet]]
name = "Weather"
icon = "weather-clear"
command = "curl -s 'wttr.in/?format=%t'"
interval_ms = 600000
format = "{output}"
tooltip_command = "curl -s 'wttr.in/?format=3'"
on_click = "foot -e curl 'wttr.in'"
```

Fields:

| Field             | Required | Meaning                                                       |
| ----------------- | -------- | ------------------------------------------------------------- |
| `name`            | yes      | identifier and fallback label                                 |
| `command`         | yes      | command whose stdout is displayed                             |
| `interval_ms`     | no       | run interval, 0 means once at startup                         |
| `icon`            | no       | icon name                                                     |
| `format`          | no       | template around `{output}`, default `{output}`                |
| `tooltip_command` | no       | command for the tooltip, run on hover or on the same interval |
| `on_click`        | no       | command to run on left click                                  |
| `max_chars`       | no       | truncate the output                                           |

Rules:

- Run the command off the UI thread.
- Keep the previous output on failure. Do not blank the applet because a script failed once.
- Trim trailing whitespace and cap the output length.
- Kill a command that exceeds a timeout, default 5 seconds.
- Do not run a command if the applet is not on a visible bar.
- Do not interpolate the output into a shell. Format it as text.
- The applet is a `gtk::Button` with an icon and a label. It uses the existing `square-icon` or a new `custom-applet` class.
- Place applets in the bar right section before the quick settings button, or in a configured order.

This is an extension point, not a plugin runtime. Load applets from one TOML table and keep them in the bar configuration.

## What not to do

- Do not port `AstalIO.Variable`, `AstalIO.Process`, or `AstalIO.Time`. They are deprecated.
- Do not add a scripting language or an embedded interpreter.
- Do not execute provider output as a command.
- Do not allow a provider to control the shell, read the notification store, or access the core state.
- Do not make the app launcher depend on Hyprland to spawn custom entries.

## UI

App launcher:

- Custom entries appear like normal apps.
- A script provider shows its configured icon in the search bar.
- A provider result launches through the same path as an app.

Bar:

- A custom applet shows an icon and a label.
- A long output is ellipsized and has a tooltip with the full text.
- A failing applet keeps its last output and logs once per failure streak.
- Reload applets when the config file changes.

## Definition of done

- A custom TOML entry appears in the launcher and launches through the detached spawn helper.
- A script provider returns JSON lines and the results launch correctly.
- A provider timeout returns no results and does not freeze the launcher.
- A bar applet displays command output and refreshes on its interval.
- A failed applet keeps the previous output.
- Config reload updates entries, providers, and applets without restarting the shell.
- No custom script runs on the GTK thread.
- The schema supports the current custom entries and the separate applet or script patch once that patch is available.
