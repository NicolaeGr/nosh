# Menus

A menu is one view plus one source. A source is a function of the query. Everything renders inside the nosh shell process, not in rofi or another external tool. The app launcher, a wallpaper picker, a clipboard history, and a power menu all use this engine.

Menus live in `~/.config/nosh/menus.toml`, keyed by id under `[menu.<id>]`. Additional menus can live in `~/.config/nosh/menus.d/*.toml`. `app_launcher.toml` keeps app entries and applets only. The `apps` builtin reads it, so custom app entries still work.

## Configuration

The file requires `schema = 1` at the top.

### Files and drop-ins

On first start, when `~/.config/nosh/menus.toml` is missing, nosh writes a template there and creates `~/.config/nosh/menus.d/`. The template defines the `apps` menu explicitly, so the running configuration is visible rather than implicit.

Every `*.toml` file under `menus.d/` is loaded in file name order. Each drop-in is a complete menu file with its own `schema = 1` line and one or more `[menu.<id>]` tables. Duplicate ids across files are an error, because a silent override would make the result depend on file order. Files with other extensions, such as `<id>.css`, are ignored by the menu loader.

Menu ids may contain ASCII letters, digits, dashes, and underscores. The id is used in the layer-shell namespace, the CSS class, and the per-menu stylesheet name, so path separators are rejected.

```toml
schema = 1

[menu.wallpapers]
title = "Wallpapers"

[menu.wallpapers.window]
namespace = "nosh-wallpapers"
anchors = ["center"]
layer = "overlay"
keyboard = "exclusive"
width = 900
height = 640

[menu.wallpapers.view]
type = "grid"
columns = 4
cell_width = 200
gap = 12
side_preview = false

[menu.wallpapers.view.item]
layout = "card"

[menu.wallpapers.source]
type = "builtin"
name = "files"
search = "client"

[menu.wallpapers.source.options]
path = "~/Pictures/wallpapers"
extensions = ["png", "jpg", "jpeg", "webp"]
recursive = false

[menu.wallpapers.action]
command = ["swww", "img", "{item.path}"]
after = "close"

[menu.wallpapers.actions]
"Ctrl+c" = { command = ["wl-copy", "{item.path}"] }

[menu.wallpapers.context_menu]
"Open folder" = { command = ["xdg-open", "{item.dir}"] }
```

Every menu has `title` and `source`. The other sections are optional.

### Window

| Key         | Default          | Meaning                                           |
| ----------- | ---------------- | ------------------------------------------------- |
| `namespace` | `nosh-menu-<id>` | layer-shell namespace                             |
| `anchors`   | `["center"]`     | any of `top`, `bottom`, `left`, `right`, `center` |
| `layer`     | `overlay`        | `background`, `bottom`, `top`, `overlay`          |
| `keyboard`  | `exclusive`      | `none`, `on_demand`, `exclusive`                  |
| `width`     | see below        | window width in pixels                            |
| `height`    | 600              | window height in pixels                           |

The width rule is ordered. `window.width` wins. Without it, `cell_width * columns` decides the width. Without either, the view gets its default, 560 for a list and 800 for a grid.

### View

`type` is `list` or `grid`. `columns` defaults to 1 for a list and 4 for a grid. `gap` defaults to 8.

`item.layout` is `row`, `tile`, or `card`. The layouts are fixed. They are not nested and there is no layout language.

- `row` shows an icon, a name, and a description. List rows stretch to the window.
- `tile` stacks a preview above a name.
- `card` is a tile with a description and a badge.

`side_preview` shows the selected item's preview in a pane beside the list. The pane is ignored for a grid.

### Source

A source is either a builtin or an external command.

```toml
[menu.x.source]
type = "builtin"
name = "files"
search = "client"
debounce_ms = 60
timeout_ms = 300
limit = 300
```

```toml
[menu.x.source]
type = "external"        # aliases: shell, script
command = ["python", "~/.config/nosh/scripts/wallpapers.py"]
search = "source"
```

An external source receives the debounced query as its last argument. With `command = ["python", "list.py"]`, the process runs `python list.py <query>`. Output is NDJSON on stdout. Each line is one JSON object, or a JSON array of objects.

The builtins are `apps`, `files`, and `static`.

| Builtin  | Options                                                                                             |
| -------- | --------------------------------------------------------------------------------------------------- |
| `apps`   | none. Reads XDG desktop entries and app entries from `app_launcher.toml`. Orders by launch history. |
| `files`  | `path` (required), `extensions`, `recursive`, `hidden`                                              |
| `static` | `items`, an array of item objects                                                                   |

A `static` source holds its items in the config:

```toml
[menu.power.source]
type = "builtin"
name = "static"

[[menu.power.source.options.items]]
id = "lock"
name = "Lock"
icon = "system-lock-screen"
action = { command = ["swaylock"] }

[[menu.power.source.options.items]]
id = "reboot"
name = "Reboot"
icon = "system-reboot"
action = { command = ["systemctl", "reboot"], after = "none" }
```

### Search

Search mode is declared per source. There is no mixed mode.

- `client` loads the source once, then nosh filters and sorts with a fuzzy scorer. Use it for short, static lists such as files, clipboard history, and power actions.
- `source` sends every debounced query to the source, and the source owns the order. Use it for app lists and external commands that search a large data set.

`debounce_ms` defaults to 60 and applies to `source` search. `timeout_ms` defaults to 300 and applies to external sources. `limit` defaults to 300 items per query.

The fuzzy scorer ranks three kinds of match, strongest first:

1. A contiguous substring anywhere in the name. Starting at the start of the name scores highest, then the start of a word.
2. A misspelling in a word, within one edit for queries of four to eight characters and two edits beyond that. `shatdown` finds `Shutdown`.
3. A subsequence that starts at the start of the name or a word, which is how `vscode` finds `Visual Studio Code`. Subsequences that start inside a word are rejected, because they match too much: `audio` would otherwise match `Visual Studio Code` through the `u`, `d`, `i`, and `o` of `Studio`.

The name is scored above keywords, and keywords above the description.

## Item contract

Every item has fixed display slots and a free-form field map.

```rust
pub struct MenuItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<MenuIcon>,       // Named(String) | Image(PathBuf)
    pub preview: Option<MenuPreview>, // Image(PathBuf) | Text(String)
    pub badge: Option<String>,
    pub group: Option<String>,
    pub keywords: Vec<String>,
    pub fields: BTreeMap<String, MenuValue>, // String | Number | Bool
    pub action: Option<ActionDef>,
    pub state: ItemState,             // Normal | Disabled { reason }
}
```

Only `name` is required. `fields` holds arbitrary source data, so actions can interpolate `{item.path}`, `{item.resolution}`, or `{item.exec}` without the engine knowing what a wallpaper or an app is.

The source fills `preview` when it has an image. The view never searches the data for an image. An item without a preview shows a placeholder or its icon.

### External items

An external source emits JSON objects. The default template maps the conventional field names:

```json
{
  "path": "/home/user/Pictures/cyberpunk.png",
  "name": "Cyberpunk City",
  "description": "4K city",
  "thumbnail": "/home/user/.cache/wallpapers/cyberpunk.png",
  "badge": "4K",
  "duration": 120
}
```

That object becomes an item with `name` from `name`, `description` from `description`, an image preview from `thumbnail`, `badge` from `badge`, and `path`, `duration`, and the rest in `fields`.

A `[menu.<id>.template]` block remaps the slots when the source uses different names:

```toml
[menu.notes.template]
id = "uuid"
name = "title"
description = "subtitle"
preview = { field = "body", kind = "text" }
icon = { field = "glyph", kind = "image" }
badge = "tag"
group = "section"
keywords = "tags"
action = "command"
```

`preview.kind` is `image` or `text`. `icon.kind` is `named` or `image`.

The `action` field may name an object with a `command` array, or an array of strings. A per-item action overrides the menu action. If neither exists, the item is disabled and its reason is shown.

### Actions

Actions use argv, never shell strings.

```toml
[menu.x.action]
command = ["swww", "img", "{item.path}"]
keep_open = false
after = "close"
launch = "auto"
```

Tokens are `{item.<field>}`, `{item.id}`, `{item.name}`, `{item.description}`, `{item.badge}`, `{item.group}`, and `{query}`. Unknown tokens stay literal. Values come from `fields` first, then the display slots.

`after` is `close`, `reload`, or `none`. `keep_open = true` keeps the window open and overrides `after`. Modifier actions (`[menu.<id>.actions]`) and context menu entries (`[menu.<id>.context_menu]`) use the same action type.

`launch` decides how the process starts:

- `auto` (default) tries `uwsm app -t service -- <argv>`, then `systemd-run --user --scope --quiet -- <argv>`, then a direct spawn.
- `uwsm` and `systemd-run` try only that wrapper, then a direct spawn.
- `detached` starts the process directly. Use it when the action needs nosh's working directory or must stay a direct child.

UWSM is detected from `UWSM_WAIT_VARNAMES` or `UWSM_FINALIZE_VARNAMES` in the environment. The systemd path needs `systemd-run` and the user manager socket at `$XDG_RUNTIME_DIR/systemd/private`. Desktop entries generated by the `apps` source use `auto`.

When a shell is needed, name it explicitly and pass item data as positional arguments:

```toml
command = ["sh", "-c", "swww img \"$1\"", "sh", "{item.path}"]
```

Do not interpolate item data into the shell string.

## Failure behavior

| Failure                           | Behavior                                        |
| --------------------------------- | ----------------------------------------------- |
| name missing or not a string      | skip the item, log once per query               |
| invalid JSON line                 | skip the line, keep the rest                    |
| missing image                     | keep the item, show a placeholder               |
| action field missing              | disable the item, show the reason               |
| duplicate id                      | suffix the id with `-2`, `-3`, and log          |
| source exits nonzero or times out | keep the previous results, show an error footer |
| output over the limit             | kill the process, keep the first N items        |

Limits are 64 KiB per line, 4 MiB total output, and 300 items per query.

## Focus

The search bar has focus by default.

- Down or Tab moves into the items at index 0.
- Up at row 0 returns to the search bar.
- Arrow keys move inside the list or grid while it has focus.
- A printable key without Ctrl, Alt, or Super moves focus back to the search bar and inserts the character at the cursor. This works wherever focus sits, on the list, the grid, or a row inside them.
- Backspace outside the search bar does the same and removes the character before the cursor.
- Enter activates the highlighted item. Escape closes the menu.
- Typing sends `MenuQuery`. The core debounces source searches and answers client searches from the cached list.

## Core and UI split

`nosh-core` owns config parsing, sources, search, validation, actions, and spawning. `nosh-ui` owns rendering, focus, and CSS.

Core modules live under `crates/nosh-core/src/menu/`:

- `model.rs`: `MenuDef`, `MenuItem`, `SourceDef`, `ViewDef`, `ActionDef`, and `Template`.
- `config.rs`: parsing, validation, defaults, and the built-in `apps` menu.
- `source.rs`: the `MenuSource` trait, the builtin registry, and the external NDJSON runner.
- `query.rs`: fuzzy scoring and client-side ordering.
- `usage.rs`: the launch log and the decayed score used to order `apps`.
- `action.rs`: token expansion, the UWSM/systemd/detached launch chain, and process reaping.
- `service.rs`: sessions, debouncing, generations, and activation.

The new commands are `MenuOpen(id)`, `MenuQuery { id, query }`, `MenuActivate { id, index }`, `MenuReload(id)`, and `MenuClose(id)`. The service publishes `MenuEvent::Opened`, `Results`, `Failed`, `Closed`, `ActionFailed`, and `ActionDone`. `NoshPaths::menus_config()` returns `~/.config/nosh/menus.toml`.

## UI

`MenuManager` keeps one layer-shell window per open menu id. A window contains a search entry, a virtualized `gtk::ListView` or `gtk::GridView`, and an error footer.

The item widgets use the classes `MenuItem`, `MenuItemRow`, `MenuItemTile`, `MenuItemCard`, `MenuItemMedia`, `MenuItemTitle`, `MenuItemDescription`, and `MenuItemBadge`. The window and root use `Menu` and `MenuRoot`. The entry uses `MenuSearch` and the footer uses `MenuError`.

Style the focusable GTK node, not an inner box. Do not use GTK CSS `:has()`, web `object-fit`, or browser class names.

The app launcher is the `apps` menu. `nosh menu apps` opens it. The generated template uses a two-column grid with row items and a window twice as wide as it is tall.

The window also carries the class `menu-<id>`, so per-menu rules can target one menu without affecting the others.

## Styling

The built-in menu rules live in `crates/nosh-ui/resources/menu.css`. They derive opaque `@menu_*` colours from `@theme_bg_color`, `@theme_fg_color`, and `@accent_bg_color`, so the menu follows the system accent. They keep the bar's corner shape, border width, shadow, and inset highlight. `@bar_accent` is still used for badge text. Corners use the same asymmetric radius as the bar islands, with `0 1em 0 1em` on the root and smaller steps inside.

The opacity rule is deliberate. Only `.MenuRoot` is translucent, at `alpha(@theme_bg_color, 0.82)`. The entry, hover, selection, badge, and media backgrounds use opaque `mix()` colours, so they do not stack alpha on top of the panel. Normal items have no background and show the panel. Disabled items use opacity 0.5 as a state cue.

On first start nosh writes `~/.config/nosh/style.css` and creates `~/.config/nosh/menus.d/`. The user stylesheet is loaded above the built-in sheet. If it contains `/* nosh:builtin-styles */`, that marker is replaced with the complete built-in stylesheet at load time. Add rules after the marker to override the built-in values. Remove the marker to stop including the built-in rules in that provider.

Each menu can add a stylesheet next to its definition: `~/.config/nosh/menus.d/<id>.css`. A menu defined in `menus.toml` can still have a file here. The file is loaded once per process, above the general user stylesheet. Scope the rules with the window class:

```css
.menu-wallpapers .MenuItemTitle {
  font-size: 1.1em;
}
```

Per-menu files are read when the menu first opens. A shell restart is needed after editing them. The general stylesheet is read at startup.

## Ordering by launch history

The `apps` builtin records each successful launch in `~/.local/share/nosh/app_usage.json`. Per app it keeps the last 12 launch times. The score is the sum of `0.5 ^ (age / 3 days)`, so a launch loses half its weight every three days.

This favours recent behaviour over lifetime totals. An app launched four or five times today passes one that has been launched daily for two weeks. A plain frequency counter would need hundreds of launches to make the same change.

With an empty query the menu sorts by score, then by name. Apps with no history follow the used ones alphabetically. With a query, the source scores a match first and adds a small usage bonus, so history breaks near-ties without hiding a better name match.

## Deliberate omissions

- There is no migration from the old Vala SQLite usage database. The file format and path are different.
- Remote preview images are out of scope. Preview paths are local files.
- Hyprland keybindings are not registered automatically. Bind `nosh menu <id>` in the compositor.
- There is one `gtk::Application`. There is no separate menu daemon.

## Definition of done

- `menus.toml` parses with clear errors for an unknown field, a bad schema, a builtin without a name, and an external source without a command.
- A missing `menus.toml` is written from the template, and `menus.d/` is created.
- Drop-ins merge in file name order, and duplicate ids are rejected.
- The user stylesheet marker expands to the built-in stylesheet.
- Per-menu CSS can target `.menu-<id>`.
- The `apps`, `files`, and `static` builtins work, including client filtering.
- The `apps` builtin orders by launch history, and recent launches outweigh old totals.
- Source search debounces, cancels stale work, and ignores late generations.
- Actions expand argv and spawn without a shell.
- The failure table holds.
- `nosh menu apps` opens the launcher.
- The wallpaper example opens, renders a grid of cards, filters, and sets a wallpaper.
- `cargo test -p nosh-core` passes.
