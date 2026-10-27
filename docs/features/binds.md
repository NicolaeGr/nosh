# Binds

Menus can declare a keybind. nosh registers it over the Hyprland command socket at runtime. No generated file, no `source` line, and no `dofile` line.

## Declaring a menu bind

```toml
[menu.apps.bind]
chord = "SUPER, Space"
forced = true
```

`forced` is optional and defaults to false.

## Media keys

nosh binds the standard media keys without any configuration:

| Key                     | Command                |
| ----------------------- | ---------------------- |
| `XF86AudioRaiseVolume`  | `nosh volume-up`       |
| `XF86AudioLowerVolume`  | `nosh volume-down`     |
| `XF86AudioMute`         | `nosh volume-mute`     |
| `XF86AudioMicMute`      | `nosh mic-mute`        |
| `XF86MonBrightnessUp`   | `nosh brightness-up`   |
| `XF86MonBrightnessDown` | `nosh brightness-down` |

The keysym names are fixed by the XDG keyboard specification. These binds are forced, because an existing bind would take over the key and the change indicator would not appear. nosh logs each override. They repeat while the key is held.

## Chord syntax

`chord` is `"MODIFIERS, KEY"`. Modifiers are `SUPER`, `SHIFT`, `CTRL`, `ALT`, `CAPS`, and `MOD2` through `MOD5`. Separate several modifiers with spaces or `+`. The key can be any Hyprland keysym, for example `Space`, `Return`, or `F1`. Keys are case-insensitive in config and nosh normalizes them for Hyprland.

## How registration works

nosh reads the current binds from `j/binds`. A chord owned by another entry is skipped unless `forced = true`. A forced bind is installed and the conflict is logged.

Managed binds carry the description `nosh ...` under the Lua provider. Under hyprlang they have dispatcher `exec` and an argument that is `nosh` or starts with `nosh `. nosh removes managed binds that are no longer installed, then unbinds and binds each installed chord. Unbinding before binding makes a repeated sync idempotent, and a forced bind replaces the user's existing bind.

Bind commands use the absolute path of the running binary, not `nosh` from `PATH`.

With the Lua provider nosh sends `eval` requests using `hl.bind` and `hl.unbind`:

```text
eval hl.unbind("SUPER + SPACE")
eval hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("/path/to/nosh menu apps"), { description = "nosh menu apps" })
```

A repeating bind adds `repeating = true` to the options table.

With the hyprlang provider nosh sends `keyword` requests:

```text
keyword unbind "SUPER, SPACE"
keyword bind "SUPER, SPACE, exec, /path/to/nosh menu apps"
```

Repeating binds use `keyword bindr` instead of `keyword bind`.

A manual `nosh ...` bind is managed. If no menu or media key installs that chord, nosh removes it.

## When sync runs

Sync runs at startup, after a reconnect, and after every `configreloaded` event, because both a restart and a reload drop or duplicate runtime binds.

## Limits

Menu binds and the standard media keys are registered today. Other commands need a manual `bind =` line until a `[binds]` table exists.
