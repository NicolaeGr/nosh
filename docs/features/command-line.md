# Command line and single instance

Current files: `src/App.vala`, `src/utils/RequestHandler.vala`, `src/utils/system_control/SystemControlManager.vala`.

## Current behavior

`App` extends `Gtk.Application`:

- Stable id `com.nicolaegr.nosh`.
- Dev id `com.nicolaegr.nosh.dev`.
- Flags `HANDLES_COMMAND_LINE`.
- Sets the program name to `nosh`.

`command_line`:

- Gets the arguments.
- If the command is remote and has at least two arguments, passes `argv[1]` to `RequestHandler.handle_command`.
- If handled, returns 0.
- If not handled, prints `Unknown command or failed to handle command. \nIf you intended to start the main instance, it is already running.\n` and returns 0.
- If the command is local, initializes CSS.
- In a non-stable build, runs `systemctl --user stop nosh`.
- Creates the bar, notifications, quick settings, app launcher, and change indicator windows.
- Adds them to the application.
- Presents the bar.
- In a non-stable build, on bar close request, runs `systemctl --user start nosh` and returns `false`.

`RequestHandler` handles:

| Command              | Action                                              |
| -------------------- | --------------------------------------------------- |
| `volume-up`          | `volume.increase()`                                 |
| `volume-down`        | `volume.decrease()`                                 |
| `brightness-up`      | `brightness.increase()`                             |
| `brightness-down`    | `brightness.decrease()`                             |
| `brightness-dim`     | save the current state and dim all displays         |
| `brightness-restore` | restore the saved brightness state                  |
| `menu <id>`          | open the named menu from `menus.toml`               |
| `monitor-cycle`      | cycle the target monitor through the layout presets |
| `power-profile`      | cycle the active power profile                      |

`monitor-cycle` accepts an optional connector or index, for example `nosh monitor-cycle DP-2` or `nosh monitor-cycle 2`. Without an argument it targets the next non-primary monitor. `power-profile` cycles `power-saver -> balanced -> performance` using the available profiles from the daemon.
| anything else | warning and return false |

The brightness and volume steps are 5 percent and 0.05 respectively. `volume-up` accepts values up to 1.5. `brightness-up` clamps to 100.

## Rust

Use `gtk::Application` with:

```rust
Application::builder()
    .application_id(if cfg!(feature = "dev") {
        "com.nicolaegr.nosh.dev"
    } else {
        "com.nicolaegr.nosh"
    })
    .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
    .build()
```

Connect `command-line`. The `ApplicationCommandLine` gives the arguments and the `is_remote()` flag.

### Mapping

```rust
match command {
    "volume-up" => core.send(Command::AudioStepVolume(0.05)),
    "volume-down" => core.send(Command::AudioStepVolume(-0.05)),
    "brightness-up" => core.send(Command::BrightnessStep(5.0)),
    "brightness-down" => core.send(Command::BrightnessStep(-5.0)),
    "brightness-dim" => core.send(Command::BrightnessDim { value: 10.0 }),
    "brightness-restore" => core.send(Command::BrightnessRestore),
    "menu" => ui.open_menu(&id),
    "monitor-cycle" => core.send(Command::MonitorCycle { target: "next".into(), mode: None }),
    "power-profile" => core.send(Command::PowerProfileCycle),
    _ => { eprintln!("Unknown command or failed to handle command. \nIf you intended to start the main instance, it is already running.\n"); }
}
```

Return `0` in every case, matching the current behavior. Do not use a nonzero exit code for an unknown command. Keybinds and scripts may depend on it.

### Implemented in the Rust build

The Rust build wires these commands:

- `volume-up` and `volume-down`
- `brightness-up` and `brightness-down`
- `brightness-dim` and `brightness-restore`
- `menu <id>`
- `monitor-cycle [target] [mode]`

`power-profile` is not implemented yet. Unknown commands print the existing message and exit 0.

`menu <id>` opens any configured menu. The app launcher is the `apps` menu, so `nosh menu apps` opens it. The UI sends `Command::MenuOpen(id)` and the core reports `MenuEvent::Opened` with the definition. The window is created from that event, so the menu definition stays in one place.

### Local startup

On the first local invocation:

1. Initialize logging.
2. Start the core.
3. Load CSS.
4. Create the windows.
5. Present the bar.

The other windows start hidden. The core snapshot fills them.

### Dev behavior

Behind the `dev` feature:

- Run `systemctl --user stop nosh` before creating windows.
- On the bar close request, run `systemctl --user start nosh`.
- Add `secondary-instance` to the bar.

Do not run `systemctl` in a stable build. Log a warning if the command fails; do not abort startup.

### Single instance

`GApplication` already provides single-instance behavior. A second `nosh volume-up` activates the first process through `command_line`. Do not add a second socket or D-Bus service for this.

If a future command needs a reply, add a `oneshot` and send the result back through the `ApplicationCommandLine`. Do not make the GTK thread block.

## Definition of done

- `nosh` starts one instance and shows the bar.
- A second `nosh volume-up` changes the running instance and exits 0.
- `nosh brightness-down` changes brightness.
- `nosh brightness-dim` saves and dims all displays; `nosh brightness-restore` restores them.
- `nosh menu apps` opens the app launcher in the running instance on the current monitor.
- `nosh menu <id>` opens the named menu in the running instance.
- `nosh monitor-cycle` applies the next layout preset to the next monitor.
- `nosh power-profile` cycles the active power profile.
- An unknown command prints the current message and exits 0.
- The stable and dev application ids are correct.
- A dev build stops and restarts the stable service; a stable build does not.
- The command handler never blocks the GTK thread.
