# System tray

Current files: `src/bar/widgets/SysTray.vala`, `astal-source/lib/tray/src/tray.vala`, `tray-item.vala`, `watcher.vala`.

## Current behavior

`SysTray` is a `Gtk.Box` with CSS class `SysTray`. It gets `AstalTray.get_default()`, connects `item_added` and `item_removed`, and keeps two hash tables: one for widgets and one for item signal handlers.

`add_item(id)`:

- Ignores an item that already exists.
- Gets the `TrayItem`.
- Ignores it if both `gicon` and `menu_model` are null.
- Creates a `Gtk.MenuButton` with CSS classes `menu-icon`, `m-0`, `p-0`.
- Sets the menu model to `item.menu_model`.
- Creates a `Gtk.Image` and binds `gicon`.
- Binds `tooltip-markup` to the button.
- Binds `menu-model` to the button.
- Inserts `item.action_group` into the button under the name `dbusmenu`.
- Reconnects the action group when `item.notify["action-group"]` fires.
- Appends the button.

`remove_item(id)` removes the widget, drops the hash entries, and disconnects the item handler.

The tray library:

- Owns `org.kde.StatusNotifierWatcher` on the session bus.
- Also acts as a host by proxying an existing watcher.
- Registers `RegisterStatusNotifierItem` and parses the service and object path.
- Removes items when the bus name loses its owner.
- Creates a `TrayItem` proxy with `org.kde.StatusNotifierItem`.
- Reads all properties, listens for `NewTitle`, `NewIcon`, `NewAttentionIcon`, `NewOverlayIcon`, `NewToolTip`, and `NewStatus`.
- Builds a `GdkPixbuf` from `IconPixmap` when no icon name is available.
- Uses `appmenu-glib-translator` to turn `com.canonical.dbusmenu` into a `GMenuModel` and `GActionGroup`.

## Core

Add a `TrayService` to `nosh-core`. It implements the StatusNotifier watcher and host side. It is not a proxy client.

Watcher role try tree:

| Order | Backend               | Probe                                   | Behavior                                 |
| ----- | --------------------- | --------------------------------------- | ---------------------------------------- |
| 1     | `OwnWatcherBackend`   | can own `org.kde.StatusNotifierWatcher` | Full watcher and host                    |
| 2     | `ProxyWatcherBackend` | another owner exists                    | Proxy the watcher and register as a host |
| 3     | `NoTrayBackend`       | always                                  | No tray section                          |

Use the try tree from [backends.md](../backends.md). Do not start a second watcher when another process already owns the name.

### Watcher

Implement the `org.kde.StatusNotifierWatcher` interface at `/StatusNotifierWatcher`:

```text
method RegisterStatusNotifierItem(s service)
method RegisterStatusNotifierHost(s service)
property RegisteredStatusNotifierItems as
property IsStatusNotifierHostRegistered b = true
property ProtocolVersion i = 0
signal StatusNotifierItemRegistered(s)
signal StatusNotifierItemUnregistered(s)
```

Registration parsing, in order:

1. If `service` starts with `/`, use the sender bus name and that path.
2. Else if `service` contains `/`, split at the first `/`, use the first part as the bus name and `"/" + rest` as the path.
3. Else use `service` as the bus name and `/StatusNotifierItem` as the path.

Track the item ID as `bus_name + path`. Watch `NameOwnerChanged`. When the owner disappears, remove the item and emit the unregistered signal. If another process already owns the watcher name, act as a host by proxying the existing watcher and registering this process with `RegisterStatusNotifierHost`.

### Item model

```rust
pub struct TrayItemModel {
    pub item_id: String,
    pub bus_name: String,
    pub object_path: String,
    pub id: String,
    pub title: String,
    pub status: TrayStatus,
    pub category: TrayCategory,
    pub tooltip_title: String,
    pub tooltip_description: String,
    pub icon_name: Option<String>,
    pub icon_theme_path: Option<PathBuf>,
    pub icon_pixmap: Option<IconPixmap>,
    pub menu_path: Option<OwnedObjectPath>,
    pub is_menu: bool,
}
```

`TrayStatus` is `Passive`, `Active`, or `NeedsAttention`. `TrayCategory` is `ApplicationStatus`, `Communications`, `SystemServices`, or `Hardware`.

Read properties with `GetAll` on `org.kde.StatusNotifierItem`:

- `Id`, `Category`, `Title`, `Status`, `ToolTip`, `IconThemePath`, `ItemIsMenu`, `Menu`, `IconName`, `IconPixmap`, `AttentionIconName`, `AttentionIconPixmap`, `OverlayIconName`, `OverlayIconPixmap`.

Listen for the `New*` signals and refresh with a short debounce, as the Vala code does. Do not refresh synchronously from the signal handler.

### Icon

If `icon_name` is a file path, use a file icon. If `icon_theme_path` is set, search that path recursively with a bounded depth for the name, then fall back to a themed icon. If there is no name, use the largest pixmap.

Decode `IconPixmap` as ARGB32 in network byte order, which is the byte order BlueZ and StatusNotifierItem use. The Vala code rotates bytes:

```text
[alpha, red, green, blue] -> [red, green, blue, alpha]
```

Reject a pixmap with width or height <= 0 and cap width and height at a sane maximum, for example 512. Do not allocate from an untrusted size.

### Menu

Do not use `appmenu-glib-translator` from the vala-panel-appmenu project. The user explicitly does not want that dependency. At most, copy its translation logic. Implement DBusMenu in Rust with `zbus`.

Protocol:

- Proxy `com.canonical.dbusmenu` at the item's menu path.
- Call `GetLayout(0, -1, ["label", "enabled", "visible", "type", "children-display", "toggle-type", "toggle-state", "icon-name", "shortcut"])`.
- Call `AboutToShow(0)` before opening.
- Subscribe to `LayoutUpdated` and `ItemsPropertiesUpdated`.
- Call `GetGroupProperties` or `GetProperty` for a targeted refresh when the compositor supports it.
- Build a `gio::Menu` with one action per item, named `dbusmenu.<id>`.
- Map separators, submenus, checkbox and radio items, enabled and visible state.
- On activation, call `Event(id, "clicked", Variant, timestamp)`.
- On open, call `AboutToShow`.
- Call `Event(id, "hovered", ...)` only if the item asks for it. Most items do not.
- Handle `LayoutUpdated` by rebuilding the model and updating the menu button. Keep the menu open if possible.
- Handle `ItemsPropertiesUpdated` by updating labels, enabled state, and toggle state in place. Apply the removed-properties half by resetting those fields to their defaults.
- Bound the layout parser: reject entries deeper than 32 levels and more than 2048 entries per item.

The implementation is a few hundred lines plus tests. Budget for:

- Menu item types: standard, separator, submenu, checkbox, radio.
- Nested submenus.
- Toggle state and the `toggle-state` property.
- Icons in menu items. GTK4 menu items do not render icons in a `PopoverMenu`. If icons are required, use a custom popover with rows instead of a `GMenuModel`.
- A menu that changes while it is open.
- A tray item that never answers. Use a timeout and fall back to `Activate`.

Do not implement both a C translator and a Rust client. The Rust client is the only path.

### Activation and focus

A left click must launch the application when it has no window and focus its window when it does. Do not track clicks in the UI or the core. The core asks the compositor on every left click:

1. Read the client list.
2. Pick a window for the item. The tiers are the process that owns the item's bus name, the window class matching the item id, the icon name, the last segment of the well-known bus name, and finally the item title. Inside a tier the most recently focused mapped, non-hidden window wins.
3. If a window matches and sits on the currently focused workspace, dispatch `focuswindow` for its address and stop.
4. Otherwise call `Activate(x, y)`, which lets the item launch or present itself. For a window on another workspace this marks that workspace urgent instead of switching to it.

Any focus failure, including a compositor that is not Hyprland, falls back to `Activate`. The cost is one `j/clients` and one `j/activeworkspace` query per left click on an item with a window.

A right click opens the context menu when the item has a `Menu` path or a loaded menu. Without one it calls `SecondaryActivate(x, y)`. A middle click always calls `SecondaryActivate`.

Scroll calls `Scroll(delta, "vertical")` or `"horizontal"` and is available on every item, with or without a menu.

## UI

`SysTray` is a `gtk::Box` with class `SysTray`.

For each item:

- A `gtk::Button` with classes `menu-icon`, `m-0`, `p-0`.
- A child `gtk::Image` bound to the item icon.
- `set_tooltip_markup` from the tooltip.
- A `gtk::PopoverMenu` parented to the button, sharing a `gio::Menu` and a `dbusmenu` action group with the core model.
- A DBusMenu separator becomes a section boundary. GTK draws a separator between adjacent sections. Do not append an item with no label and no action, which renders as an empty row.
- The button stays in place when a menu appears or disappears; only the menu model changes.
- If the item is removed, remove the button and drop the action group.

Input is one gesture controller per button:

- Primary button: send `Command::TrayActivate { item_id, x, y }`, or close the popover when it is open.
- Secondary button: open the popover and send `Command::TrayMenuAboutToShow`, or send `Command::TraySecondaryActivate` when the item has no menu.
- Middle button: send `Command::TraySecondaryActivate`.
- Scroll: send `Command::TrayScroll`.

Which button activates and which opens the menu is a UI decision. The core only resolves focus and performs D-Bus calls.

The UI must not call D-Bus or parse menu items. It receives an opaque menu model and action group from the core.

### Icon rendering

Use `gtk::Image::from_gicon` for themed and file icons. For a raw pixmap, create a `gdk::Texture` from the decoded RGBA bytes and use `gtk::Image::from_paintable`.

### CSS

Keep `.SysTray` and `.square-icon, .menu-icon button` from `src/style/_blocks.scss`. The `menu-icon` button has a transparent background and a 32x32 minimum.

## Defects to fix

- Ignore zero-sized pixmaps and do not call `GdkPixbuf.from_bytes` with them.
- Do not use synchronous D-Bus calls for `AboutToShow`, menu layout, or activation.
- Do not recursively walk an unbounded icon theme path. Cache results.
- Do not refresh item properties synchronously on every `New*` signal. Debounce.
- Handle a registration string that combines a service and an object path.
- Remove items when the bus name owner disappears.
- Do not block the GTK thread when a tray item is slow or unresponsive.

## Definition of done

- A StatusNotifierItem with an icon and a menu appears in the bar.
- Left click launches the application, and a later left click focuses its existing window.
- Right click opens the context menu; menu actions fire the correct DBusMenu event.
- A tooltip appears with the item title and description.
- An item with no menu calls `SecondaryActivate` on a right click.
- Removing the item's bus name removes the button.
- A zero-sized pixmap falls back to the icon name or a missing-image icon without a crash.
- Tray operations do not block the GTK thread.
- The core owns the watcher, item proxies, menu model, and focus resolution. The UI owns only the button and popover.
