# Menus

A menu is one view plus one source. Menus live in `$XDG_CONFIG_HOME/nosh/menus.toml` or in drop-ins under `$XDG_CONFIG_HOME/nosh/menus.d/`.

## Files and ids

Every menu file starts with `schema = 1` and can hold one or more `[menu.<id>]` tables.

```toml
schema = 1

[menu.wallpapers]
title = "Wallpapers"

[menu.power]
title = "Power"
```

The id is what `nosh menu <id>` opens, what the `menus` source lists, and what the per-menu stylesheet is named after. A single TOML file can define several menus, and each still gets its own `menus.d/<id>.css`. The TOML file name only decides load order.

## Sources

A source returns items for a query. The source kind decides where the items come from.

### apps

The `apps` source combines XDG desktop entries with the files in `apps.d/`. It reads launch history from `app_usage.json` and sorts by it.

```toml
[menu.apps.source]
type = "builtin"
name = "apps"
search = "source"
```

`search = "source"` sends every query to the source. `search = "client"` loads the list once and lets nosh filter and sort it.

Desktop entries with `Terminal=true` use the menu terminal wrapper:

```toml
[menu.apps.terminal]
command = ["kitty", "--"]
```

### files

The `files` source lists files in one directory. It works well for wallpapers, screenshots, or notes.

```toml
[menu.wallpapers.source]
type = "builtin"
name = "files"
search = "client"

[menu.wallpapers.source.options]
path = "~/Media/images/Wallpapers"
extensions = ["png", "jpg", "jpeg", "webp"]
```

Each item gets the file name and a `path` field. Use `{item.path}` in an action.

### menus

The `menus` source lists the other configured menus, so a launcher can open a menu picker. The generated `menus.toml` uses it for the `menus` menu.

```toml
[menu.switcher.source]
type = "builtin"
name = "menus"
search = "client"
```

Menus marked `hidden = true` stay out of this list.

### static

The `static` source keeps the complete item list in the menu file.

```toml
[menu.power]
title = "Power"

[menu.power.view]
type = "list"

[menu.power.source]
type = "builtin"
name = "static"

[menu.power.source.options]

[[menu.power.source.options.items]]
id = "lock"
name = "Lock"
description = "Lock the session"
icon = "system-lock-screen"
action = { command = ["nosh", "lock"], after = "close" }

[[menu.power.source.options.items]]
id = "reboot"
name = "Reboot"
icon = "system-reboot"
action = { command = ["systemctl", "reboot"], after = "none" }
```

An item can carry an inline `action`. It can also carry `preview = { text = "..." }` or `preview = { image = "/path/preview.png" }`, and an icon as a name or as `{ image = "/path/icon.png" }`.

### external

An external source runs a command and reads one JSON object per line. The debounced query is appended as the last argument.

```toml
[menu.themes.source]
type = "external"
command = ["ht", "list", "--json"]
search = "source"
timeout_ms = 2000
limit = 200
```

The source fields are:

| Field         | Meaning                                                         |
| ------------- | --------------------------------------------------------------- |
| `command`     | argv list, without a shell                                      |
| `search`      | `client` or `source`, defaults to `source` for external sources |
| `debounce_ms` | delay before a query reaches the process, default 60            |
| `timeout_ms`  | process timeout, default 300                                    |
| `limit`       | items kept from one response, default 300                       |
| `options`     | extra values passed to the source, if it reads them             |

`ht list --json` is a good example. It prints objects like this:

```json
{
  "id": "catppuccin-mocha",
  "name": "Catppuccin Mocha",
  "description": "dark",
  "thumbnail": "/path/mocha.svg",
  "badge": "active",
  "theme": "catppuccin-mocha"
}
```

## Templates

The template maps JSON fields onto the item slots. Without a template, nosh reads `id`, `name`, `description`, `icon`, `thumbnail`, `badge`, `group`, `keywords`, and `action`.

```toml
[menu.themes.template]
id = "id"
name = "name"
description = "description"
icon = { field = "icon", kind = "named" }
preview = { field = "thumbnail", kind = "image" }
badge = "badge"
group = "group"
keywords = "keywords"
```

`icon` uses `kind = "named"` for an icon theme name and `kind = "image"` for a file path. `preview` uses `kind = "image"` for a file path and `kind = "text"` for a short string. An item without a name is skipped.

Every scalar field in the JSON object is kept. That is why `{item.theme}` works even when the template does not name `theme`.

## Actions

An action is an argv list. nosh expands `{query}` and `{item.<field>}` tokens, and leaves unknown tokens alone so a typo shows up in the spawned command.

```toml
[menu.themes.action]
command = ["ht", "set", "{item.theme}"]
after = "reload"
launch = "detached"
```

| Key         | Values                                    | Meaning                                         |
| ----------- | ----------------------------------------- | ----------------------------------------------- |
| `command`   | argv list                                 | the program to run                              |
| `after`     | `close`, `reload`, `none`                 | what happens to the menu after the action       |
| `keep_open` | boolean                                   | keep the menu open after a successful action    |
| `launch`    | `auto`, `uwsm`, `systemd-run`, `detached` | how the process is started                      |
| `terminal`  | boolean                                   | run the action inside the menu terminal wrapper |

Named actions live in `[menu.<id>.actions]` and can be reached from `keys` or from an external item's `action` field. The `[menu.<id>.context_menu]` table is a second place to name an action. It does not add a right-click gesture by itself; an item or key has to name the action.

```toml
[menu.wallpapers.actions.open]
command = ["xdg-open", "{item.path}"]
after = "close"
launch = "detached"

[menu.wallpapers.keys]
"ctrl+return" = "open"
```

`context_menu` actions are reached by naming them, like any other action. The reserved name `terminal` runs the default action inside the terminal wrapper.

## Views and windows

A view is `list` or `grid`. An item layout is `row`, `tile`, or `card`.

```toml
[menu.wallpapers.view]
type = "grid"
columns = 4
cell_width = 200
gap = 12

[menu.wallpapers.view.item]
layout = "card"
```

The window table controls the layer-shell surface:

```toml
[menu.wallpapers.window]
namespace = "nosh-menu-wallpapers"
anchors = ["center"]
layer = "overlay"
keyboard = "exclusive"
width = 900
height = 640
```

Anchors are `top`, `bottom`, `left`, `right`, and `center`. Layers are `background`, `bottom`, `top`, and `overlay`. Keyboard modes are `none`, `on_demand`, and `exclusive`.

## Binding

On Hyprland, a menu can write its own bind at startup:

```toml
[menu.apps.bind]
chord = "SUPER, X"
forced = false
```

nosh skips a chord another program already owns. Set `forced = true` to take it anyway. On another compositor, bind the command yourself.

## Per-menu CSS

A menu stylesheet lives at `$XDG_CONFIG_HOME/nosh/menus.d/<id>.css` and is re-read when that menu opens.

```css
.MenuItem {
  border-radius: 8px;
}

.MenuItemTitle {
  font-weight: 600;
}
```

The file name is the menu id, not the TOML file name. If one TOML file defines `wallpapers`, `power`, and `themes`, you can write `wallpapers.css`, `power.css`, and `themes.css`. See [Styling](styling.md) for the shared stylesheet and the class names.
