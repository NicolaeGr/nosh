# GTK widgets

Pseudocode widget trees and GTK behavior for the shell. This is not a complete property list. It describes the structure, classes, focus, and interactions that matter for parity.

## Conventions

```text
Window(anchor, layer, exclusivity, keymode, namespace, monitor)
Box(orientation, spacing, classes, children)
Label(text, classes, wrap, ellipsize, max_width_chars, lines)
Image(icon_name or gicon, pixel_size)
Button(child, classes, on_click)
Switch(active, classes, on_change)
Scale(min, max, step, value, classes, on_change)
Revealer(child, transition, duration, reveal)
```

Set all widget mutation on the GTK main thread. The core sends events, the UI applies them.

## Layer-shell window

```text
create_window(kind):
  window = gtk_window()
  window.decorated = false
  window.resizable = false
  window.width_request = 1
  window.height_request = 1
  gtk4_layer_shell.init_for_window(window)
  gtk4_layer_shell.set_namespace(window, namespace)
  gtk4_layer_shell.set_layer(window, layer)
  gtk4_layer_shell.set_anchor(window, edge, true) for each anchored edge
  gtk4_layer_shell.set_keyboard_mode(window, keymode)
  gtk4_layer_shell.set_monitor(window, monitor)
  if exclusivity == EXCLUSIVE:
    gtk4_layer_shell.auto_exclusive_zone_enable(window)
  else:
    gtk4_layer_shell.set_exclusive_zone(window, -1)
  app.add_window(window)
  window.present() when the state says it should be visible
```

Do not set an exclusive zone on notifications, quick settings, the app launcher, or the change indicator.

## Bar

One bar per enabled monitor.

```text
BarWindow(monitor):
  Window(
    anchor = top | left | right,
    layer = top,
    exclusivity = exclusive,
    keymode = none,
    namespace = "nosh-topbar",
    monitor = monitor,
    classes = ["Bar", "min-h-6", "px-1", "py-0", "mx-1", "text-sm", "font-bold"]
  )
    CenterBox
      start = Box(halign=start, hexpand=true)
        Workspaces(monitor)
      center = Box(orientation=horizontal)
        Time()
        Media()
      end = Box(halign=end, hexpand=true, classes=["Right"])
        SystemStats()
        SysTray()
        IdleInhibitor()
        Battery()
        Wifi()
        Volume()
        QuickSettingsButton()
```

Dev build adds `secondary-instance` to `Bar`.

Time:

```text
Time:
  Box(classes=["Time"])
    Label(format(now, "%H:%M - %a %e"))
  every 1000 ms:
    label.text = format(now, format_string)
```

Media:

```text
Media(classes=["Media"]):
  if selected player exists:
    Label("{artist} - {title}", ellipsize=end)
  else:
    hidden
```

Quick settings button:

```text
Button(
  child = Image(icon_name="preferences-other-symbolic"),
  classes = ["square-icon"],
  on_click = toggle_quick_settings
)
```

## Workspaces

```text
Workspaces(monitor):
  Box(classes=["Workspaces", "p-1", "mr-2", "gap-1"], spacing=0)
  for workspace in sorted(normal_workspaces_for(monitor)):
    Button(
      child = Label(workspace.label),
      classes = ["min-w-8", "min-h-2", "p-0", "mx-0", "text-xs", "font-semibold", "opacity-60", "transition-all", "duration-200", "ease-in-out"],
      on_click = activate(workspace)
    )
    if workspace.focused: add_class("focused")
    if workspace.urgent: add_class("urgent")
```

Update buttons by key. Do not rebuild the whole box on every event.

## System stats

```text
SystemStats(classes=["SystemStats"]):
  Box(orientation=horizontal)
    Label(cpu_text, classes=["cpu-stat", level_class(cpu_percent)])
    Label(mem_text, classes=["mem-stat", level_class(mem_percent)])
```

Level classes:

```text
percent < 50  -> level-low
percent < 70  -> level-medium
percent < 90  -> level-high
else          -> level-critical
```

Update the text and the level class. Do not recreate labels.

## Tray

```text
SysTray(classes=["SysTray"]):
  Box()
  for item in tray_items:
    if item.has_menu:
      MenuButton(
        child = Image(gicon=item.gicon),
        menu_model = item.menu_model,
        tooltip_markup = item.tooltip_markup,
        classes = ["menu-icon", "m-0", "p-0"]
      )
      menu_button.insert_action_group("dbusmenu", item.action_group)
      on_open = item.about_to_show()
    else:
      Button(
        child = Image(gicon=item.gicon),
        tooltip_markup = item.tooltip_markup,
        classes = ["square-icon"],
        on_click = item.activate(x, y)
      )
```

Update a button in place when its icon or menu changes. Remove it when the item disappears.

## Battery

```text
Battery:
  Button(
    child = Image(icon_name=battery.icon_name),
    classes = ["square-icon"],
    tooltip = "Battery: {floor(percentage * 100)}%"
  )
```

Hide when no battery is present, or show `battery-missing-symbolic` for parity.

## Wi-Fi

```text
Wifi:
  Button(
    child = Image(icon_name=wifi.icon_name),
    classes = ["square-icon"],
    tooltip = wifi.active_ssid or "",
    visible = wifi.available
  )
```

No click action in v1.

## Volume

```text
Volume:
  Button(
    child = Image(icon_name=audio.default_sink.icon_name),
    classes = ["square-icon"],
    tooltip = "Volume: {floor(volume * 100)}%"
  )
  scroll_controller(vertical):
    up:
      if volume < 1.0: send AudioStepVolume(+0.05)
    down:
      send AudioStepVolume(-0.05)
```

The bar never adds `overamp`.

## Idle inhibitor

```text
IdleInhibitor:
  Button(
    child = Image(icon_name=icon),
    classes = ["square-icon"],
    sensitive = available,
    tooltip = tooltip,
    on_click = send IdleInhibitToggle
  )
```

Icons:

```text
unavailable -> dialog-error, "Error: Idle inhibitor not available"
active      -> caffeine-cup-full, "Active"
inactive    -> caffeine-cup-empty, "Inactive"
```

## Quick settings

```text
QuickSettingsWindow:
  Window(
    anchor = all four,
    layer = overlay,
    exclusivity = ignore,
    keymode = exclusive,
    namespace = "nosh-quick-settings",
    monitor = current_monitor,
    classes = ["QuickSettings"]
  )
    Box(orientation=vertical, hexpand=true, vexpand=true, classes=["background"])
      Card()
```

Card:

```text
Card:
  Box(orientation=vertical, classes=["card"], halign=end, valign=start, margins=(56, 16, 16, 16))
    ScrolledWindow(policy=never, never, min_width=260)
      Box(orientation=vertical)
        Connectivity()
        Separator(classes=["divider"])
        VolumeSlider()
        Microphone()
        BrightnessSlider()
        Separator(classes=["divider"])
        Toggles()
```

Background click:

```text
on click(x, y):
  if picked_widget == background:
    close_quick_settings()
```

Escape closes. Close all dropdowns when the window closes.

### Connectivity

```text
Connectivity:
  Box(orientation=vertical)
    Box(orientation=horizontal, spacing=8, classes=["item-row"], homogeneous=true)
      WiFiButton()
      BluetoothButton()
      PowerProfileButton()
      KDEConnectButton()
    Box(orientation=vertical, classes=["dropdowns-container"])
      WiFiRevealer()
      BluetoothRevealer()
      KDEConnectRevealer()
```

Only one revealer is open. Opening one closes the others.

### Large button

```text
LargeButton:
  Box(orientation=vertical)
    Button(classes=["large-button"])
      Box(orientation=vertical, spacing=4, halign=center, valign=center, margins=8)
        Image(icon_size=large, classes=["icon", enabled_or_disabled])
        Label(status, classes=["status"], justify=center)
        Label(name, classes=["QuickSettings-label"], wrap=true)
```

`set_icon_enabled(true)` adds `enabled` and removes `disabled` from the icon. `set_status(text)` updates the status label.

### Toggles

```text
QuickToggleButton(type, icon_name, label_text):
  Button(classes=["QuickSettings-toggle-button", optional "active"])
    Box(orientation=vertical, spacing=4, halign=center, valign=center, margins=8)
      Image(icon_name, icon_size=large, classes=["icon"])
      Label(label_text, classes=["QuickSettings-label"], wrap=true)
```

Toggles:

```text
DND       notifications-disabled-symbolic   DND
No Sleep  eye-slash-symbolic                No Sleep
Power     power-profile-{profile}-symbolic  Power Saver / Balanced / Performance
VPN       security-high-symbolic            VPN
```

DND and No Sleep read core state. Power reads the power profile. VPN is disabled until a VPN service exists.

### Sliders

```text
Slider(label, icon, min, max, value):
  Box(orientation=vertical, classes=["slider"], margins=(6, 12, 6, 12))
    Box(orientation=horizontal, spacing=8)
      Image(icon_name)
      Label(label)
      Label(percent, classes=["numeric"], halign=end, hexpand=true)
    Scale(min, max, step=1, draw_value=false, hexpand=true)
```

Volume uses `0..150`, class `volume`, and `overamp` above 100. Microphone uses `0..100`. Brightness uses `0..100`.

### Dropdown

```text
Dropdown:
  Revealer(transition=slide_down, duration=300, reveal=false)
    Box(orientation=vertical, classes=["dropdown"])
      Header()
      Separator(classes=["divider"])
      ScrolledWindow(policy=never, automatic, min_height=240)
        Content()
```

Wi-Fi header: label `WiFi`, switch, refresh, collapse. Bluetooth header: label `Bluetooth`, switch, refresh, settings, collapse. KDE Connect header: title, switch, refresh, settings, collapse.

Do not use the CSS `max-height` animation from the current code. Use `Revealer`.

## Notifications

```text
NotificationsWindow:
  Window(
    anchor = configured_corner,
    layer = overlay,
    exclusivity = ignore,
    keymode = none,
    namespace = "nosh-notifications-outer",
    monitor = farthest_monitor(corner),
    margins = (8 vertical, 28 horizontal),
    classes = ["Notifications-outer"]
  )
    NotificationsContainer()
```

Container:

```text
NotificationsContainer(classes=["NotificationsContainer"]):
  Box(orientation=vertical, spacing=4, classes=["notifications-stack"])
  max_visible = 3
  queue = empty
  on notification:
    if suppress_popup or dnd:
      return
    if visible_count < max_visible:
      append Card(notification)
    else:
      queue.push(Card(notification))
  on card dismissed:
    remove card
    if queue not empty:
      append queue.pop()
    if no cards:
      window.hide()
      add class "pointer-events-none"
```

Card:

```text
NotificationCard:
  Box(orientation=vertical, spacing=4, classes=["NotificationCard", "p-2", "gap-1", "rounded-2xl"])
    Box(orientation=horizontal, spacing=8, classes=["gap-2", "header"])
      Image(icon 32)
      Label(title, classes=["heading"], ellipsize=end, lines=1, hexpand=true)
      Label(time, classes=["heading"], halign=end)
      Box(orientation=horizontal, spacing=4, classes=["gap-1"])
        Button(icon=pan-down-symbolic or pan-up-symbolic, classes=["min-w-0", "min-h-0", "square-icon"])
        Button(icon=window-close-symbolic, classes=["min-w-0", "min-h-0", "square-icon"])
    Revealer(reveal=true, transition=slide_down, duration=200)
      Box(orientation=vertical, spacing=8)
        Label(body, use_markup=true, wrap=word_char, max_width_chars=40, lines=3, ellipsize=end, halign=start)
        Box(orientation=horizontal, spacing=4, classes=["gap-1", "mt-2"], homogeneous=true)
          Button(label=action.label, classes=["text-button", "rounded-md"]) for each action
  add class "notification-card-appear"
  on enter: pause timer
  on leave: resume timer
  on timeout: dismiss
```

Dismiss adds `notification-card-dismiss`, calls `NotificationDismiss`, and removes the card after 250 ms.

## App launcher

```text
AppLauncherWindow:
  Window(
    anchor = all four,
    layer = overlay,
    exclusivity = ignore,
    keymode = exclusive,
    namespace = "nosh-app-launcher",
    monitor = current_monitor,
    classes = ["AppLauncher"]
  )
    Box(classes=["background"], hexpand=true, vexpand=true)
      Box(classes=["container"], orientation=vertical, halign=center, valign=center, size=500x312)
        SearchBar()
        AppGrid()
```

Search bar:

```text
SearchBar:
  Box(orientation=horizontal, spacing=8)
    SearchEntry(placeholder="Search applications...", classes=["SearchBar"], hexpand=true)
    Image(classes=["plugin-icon"], pixel_size=24, visible=plugin_active)
```

Grid:

```text
AppGrid:
  Box(orientation=vertical, classes=["AppGrid"], vexpand=true, hexpand=true)
    ScrolledWindow(policy=never, automatic, vexpand=true, hexpand=true)
      FlowBox(
        max_children_per_line=2,
        min_children_per_line=2,
        column_spacing=8,
        row_spacing=8,
        homogeneous=true,
        selection_mode=single,
        valign=start
      )
```

Item:

```text
AppItem:
  Box(orientation=horizontal, spacing=8, classes=["AppItem"])
    Image(icon_name=app.icon or "application-x-executable", pixel_size=32)
    Label(app.name, ellipsize=end, xalign=0, hexpand=true)
```

Keyboard:

```text
SearchEntry key controller in capture phase:
  Control held: do not handle
  Escape:
    if grid_mode: exit grid mode
    else: close launcher
  Down:
    if not grid_mode: enter grid mode
    move down
  Up, Left, Right in grid mode: move
  Enter: activate selected
```

Grid movement uses two columns. Scroll the selected child into view.

## Change indicator

```text
ChangeIndicatorWindow:
  Window(
    anchor = bottom | left | right,
    layer = top,
    exclusivity = ignore,
    keymode = none,
    namespace = "nosh-change-indicator",
    monitor = current_monitor,
    classes = ["ChangeIndicator"]
  )
    Box(orientation=vertical, spacing=12, classes=["indicator-card"], halign=center, valign=end, margin_bottom=40)
      Box(orientation=horizontal, spacing=8, halign=center)
        Image(classes=["indicator-icon"])
        Label(classes=["indicator-value"])
      ProgressBar(classes=["indicator-bar"], hexpand=true)
```

Show:

```text
on user volume change:
  icon = audio-volume-medium-symbolic
  value = round(volume * 100) + "%"
  fraction = clamp(volume / 1.5, 0, 1)
  overamp = value > 100
  show for 800 ms

on user brightness change:
  icon = display-brightness-symbolic
  value = round(percent) + "%"
  fraction = clamp(percent / 100, 0, 1)
  show for 800 ms
```

A new change restarts the timer. Do not queue indicators.

## Power menu

```text
PowerMenuWindow:
  Window(
    anchor = all four,
    layer = overlay,
    exclusivity = ignore,
    keymode = exclusive,
    namespace = "nosh-power-menu",
    monitor = current_monitor,
    classes = ["PowerMenu"]
  )
    Box(classes=["background"], hexpand=true, vexpand=true)
      Card(classes=["power-card"])
        Button("Shutdown", on_click = confirm("shutdown"))
        Button("Reboot", on_click = confirm("reboot"))
        Button("Logout", on_click = confirm("logout"))
        Button("Suspend", on_click = confirm("suspend"))
        Button("Hibernate", on_click = confirm("hibernate"))
        Button("Cancel", on_click = close)
```

Confirm opens the auth dialog, then sends the matching `PowerService` command. Escape closes. Click the background to close.

## CSS class map

| Widget | Classes |
| --- | --- |
| Bar | `Bar min-h-6 px-1 py-0 mx-1 text-sm font-bold` |
| Secondary bar | `secondary-instance` |
| Workspaces | `Workspaces p-1 mr-2 gap-1` |
| Workspace button | `min-w-8 min-h-2 p-0 mx-0 text-xs font-semibold opacity-60 transition-all duration-200 ease-in-out` |
| Square icon | `square-icon` |
| System stats | `SystemStats`, labels `cpu-stat`, `mem-stat`, `level-*` |
| Quick settings | `QuickSettings`, `background`, `card`, `item-row`, `divider`, `dropdowns-container` |
| Large button | `large-button`, `icon`, `status` |
| Toggle | `QuickSettings-toggle-button`, `QuickSettings-label`, `active` |
| Slider | `slider`, `numeric`, `volume`, `overamp` |
| Dropdown | `dropdown`, `header`, `divider`, `visible` |
| Notifications | `Notifications-outer`, `NotificationsContainer`, `notifications-stack`, `pointer-events-none` |
| Notification card | `NotificationCard p-2 gap-1 rounded-2xl`, `header`, `heading`, `notification-card-appear`, `notification-card-dismiss` |
| App launcher | `AppLauncher`, `background`, `container`, `SearchBar`, `AppGrid`, `AppItem` |
| Change indicator | `ChangeIndicator`, `indicator-card`, `indicator-icon`, `indicator-value`, `indicator-bar` |
| Power menu | `PowerMenu`, `background`, `power-card` |

Do not rename these classes. Users target them in their own stylesheets.

## Focus and visibility

- The bar does not take keyboard focus.
- Quick settings, the app launcher, and the power menu grab focus when shown.
- Notifications and the change indicator never take focus.
- Hide a window before moving it to another monitor, then present it again.
- When a monitor is removed, move any current-monitor window to the new current monitor.
- When no notifications are visible, hide the notification window and add `pointer-events-none`.

## Tests

- Every window maps with the documented namespace.
- The bar reserves an exclusive zone on every monitor.
- Quick settings and the app launcher map on the current monitor.
- Notifications map on the farthest monitor for each corner.
- Escape closes quick settings and the app launcher.
- Clicking the background closes quick settings and the app launcher.
- Dropdowns use `Revealer`, not the CSS max-height hack.
- Notification cards pause on hover and resume with the remaining time.
- The power menu opens the auth dialog before sending a logind action.
