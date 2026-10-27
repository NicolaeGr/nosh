# Binds

Menus can declare a keybind. nosh registers it over the Hyprland command socket at runtime. No generated file, no `source` line, and no `dofile` line.

## Declaring a menu bind

```toml
[menu.apps.bind]
chord = "SUPER, Space"
forced = true
```

`forced` is optional and defaults to false.

## Chord syntax

`chord` is `"MODIFIERS, KEY"`. Modifiers are `SUPER`, `SHIFT`, `CTRL`, `ALT`, `CAPS`, and `MOD2` through `MOD5`. Separate several modifiers with spaces or `+`. The key can be any Hyprland keysym, for example `Space`, `Return`, or `F1`. Keys are case-insensitive in config and nosh normalizes them for Hyprland.

## How registration works

nosh reads the current binds from `j/binds`. A chord owned by another entry is skipped unless `forced = true`. A forced bind is installed and the conflict is logged.

Managed binds are the entries whose dispatcher is `exec` and whose argument starts with `nosh menu `. nosh removes managed binds that are no longer installed, then unbinds and binds each installed chord. Unbinding before binding makes a repeated sync idempotent, and a forced bind replaces the user's existing bind.

With the Lua provider nosh sends `eval` requests using `hl.bind` and `hl.unbind`:

```text
eval hl.unbind("SUPER + SPACE")
eval hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("nosh menu apps"))
```

With the hyprlang provider nosh sends `keyword` requests:

```text
keyword unbind "SUPER, SPACE"
keyword bind "SUPER, SPACE, exec, nosh menu apps"
```

A user's manual `nosh volume-up` bind is not managed and is left alone.

## When sync runs

Sync runs at startup, after a reconnect, and after every `configreloaded` event, because both a restart and a reload drop or duplicate runtime binds.

## Limits

Only menu binds are registered today. Commands outside menus need a manual `bind =` line until a `[binds]` table exists.
