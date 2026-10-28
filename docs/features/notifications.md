# Notifications

Current files: `src/notifications/Window.vala`, `widgets/NotificationsContainer.vala`, `widgets/NotificationCard.vala`, `widgets/lookup_icon.vala`, `src/notifications/Notifications.scss`, plus `astal-source/lib/notifd`.

This feature has two halves. The core is a freedesktop notification daemon and store. The UI is a stack of cards in the top-right corner. Do not merge them.

## Current behavior

### Daemon

`AstalNotifd.Notifd` is a singleton. The first instance requests `org.freedesktop.Notifications` and becomes the daemon. Later instances become proxies and read the daemon over D-Bus.

Settings live in GSettings schema `io.astal.notifd`:

| Key                  | Type              | Default                                                                                                 |
| -------------------- | ----------------- | ------------------------------------------------------------------------------------------------------- |
| `ignore-timeout`     | bool              | false                                                                                                   |
| `dont-disturb`       | bool              | false                                                                                                   |
| `default-timeout`    | int               | -1                                                                                                      |
| `notifications`      | array of variants | empty                                                                                                   |
| `server-capabilites` | string array      | action-icons, actions, body, body-hyperlinks, body-images, body-markup, icon-static, persistence, sound |

The daemon implements:

```text
org.freedesktop.Notifications
  Notify(app_name s, replaces_id u, app_icon s, summary s, body s,
         actions as, hints a{sv}, expire_timeout i) -> u
  CloseNotification(id u)
  GetCapabilities() -> as
  GetServerInformation() -> (name s, vendor s, version s, spec_version s)
  signal NotificationClosed(id u, reason u)
  signal ActionInvoked(id u, action s)
```

`GetServerInformation` returns `"notifd"`, `"astal"`, `"0.1"`, `"1.2"`.

`Notify` behavior:

- If `hints["image-data"]` exists, decode it, save it as a PNG in `$XDG_CACHE_HOME/astal/notifd`, replace it with `image-path`, and remove `image-data`.
- Remove the deprecated `image_data` and `icon_data` hints.
- If `replaces_id` exists, reuse that ID. Else assign the next integer ID starting at 1.
- Create a notification with app name, app icon, summary, body, timeout, and current Unix time.
- Copy every hint.
- Build actions from the flat string array in pairs.
- Add the notification to the store.
- Emit `notified(id, replaced)`.
- Schedule a state flush in 100 ms.
- If `ignore-timeout` is false and `expire_timeout > 0`, resolve with `EXPIRED` after that timeout.
- Return the ID.

`CloseNotification` resolves with `CLOSED`.

When a notification action is invoked, the daemon emits `ActionInvoked`, and if the notification is not resident, resolves it with `CLOSED`. `dismiss` resolves with `DISMISSED_BY_USER`, `expire` with `EXPIRED`.

The timeout used for a notification is `expire_timeout` when non-negative, otherwise `default_timeout`.

Image caching supports only 8 bits per sample RGB. It warns and drops other formats.

State flush writes every non-transient notification as a serialized variant into the GSettings `notifications` key.

### UI container

`Notifications.Window` is an `Astal.Window`:

- Anchor top and right.
- Exclusivity `IGNORE`.
- Layer `OVERLAY`.
- Visible false.
- Namespace `nosh-notifications-outer`.
- Margin top 8, right 28.
- CSS class `Notifications-outer`.

The container is a `Gtk.Box` with class `NotificationsContainer`:

- Align end and start.
- Minimum width 360, maximum width 400.
- Vertical stack, spacing 4, class `notifications-stack`.
- Maximum 3 visible cards.
- A `Queue<NotificationCard>` of waiting cards.
- On `notifd.notified`, gets the notification and creates a card.
- Iterates existing notifications on startup.
- Each card gets actions from `notification.actions`; clicking calls `notification.invoke(action.id)`.
- When a card is dismissed, removes it and shows the next queued card.
- When no cards remain, hides the window and sets a zero height request.
- Adds `pointer-events-none` when empty.

### UI card

`NotificationCard` is a vertical `Gtk.Box`:

- CSS classes `NotificationCard`, `p-2`, `gap-1`, `rounded-2xl`.
- Margin bottom 8.
- Header: icon, title, time, expand and close buttons.
- Icon: use `notification.image` if it is an existing file, else `LookupIcon.lookup(app_icon, "application-x-executable")`.
- Title: class `heading`, start aligned, expanding, one line, ellipsize end.
- Time: current `%H:%M`, class `heading`, end aligned.
- Expand button: `pan-down-symbolic` or `pan-up-symbolic`, classes `min-w-0 min-h-0 square-icon`.
- Close button: `window-close-symbolic`, same classes.
- Content: body label with markup, wrap `WORD_CHAR`, start aligned, max width 40 characters, 3 lines, ellipsize end.
- Actions row: horizontal, homogeneous, classes `gap-1 mt-2`.
- Content is inside a `Gtk.Revealer`, 200 ms, `SLIDE_DOWN`.
- Default timeout 5000 ms.
- Hover pauses the timer and resumes with the remaining time using `g_get_monotonic_time`.
- `dismiss` calls `notification.dismiss()`, adds `notification-card-dismiss`, and emits `dismissed` after 250 ms.

### LookupIcon

Searches the default icon theme for the name, then the lowercase and uppercase variants, then the fallbacks, and finally returns the original name or `application-x-executable`. It caches the theme.

### CSS

- Outer margin top 64. This conflicts with the window margin top 8. Keep the layer-shell margin and use content padding if the cards must clear the bar.
- Card background `alpha(@card_bg_color, 0.95)`, radius 16, shadow, border `alpha(@accent_bg_color, 0.2)`, blur 10, transition 200 ms.
- Hover background `@card_bg_color`, border `alpha(@accent_bg_color, 0.4)`, softer shadow.
- Appear animation `notification-bounce-in` 0.4 s.
- Dismiss animation `notification-slide-out` 0.25 s forwards.
- Header image 32x32, heading 0.875 rem and weight 600.
- Text button padding 8 16, font 0.75 rem.
- `rounded-md` 6 px, `mt-2` 8 px.

## Core

Add a `NotificationService` to `nosh-core`. It owns the daemon role, the store, DND, timeouts, and persistence.

Daemon role try tree:

| Order | Backend                      | Probe                                                   | Behavior                        |
| ----- | ---------------------------- | ------------------------------------------------------- | ------------------------------- |
| 1     | `OwnDaemonBackend`           | can request `org.freedesktop.Notifications`             | Full daemon, store, timeouts    |
| 2     | `ExistingDaemonProxyBackend` | another owner exists and answers `GetServerInformation` | Proxy notifications and actions |
| 3     | `NullNotificationBackend`    | always                                                  | Log and do not display          |

Use the try tree from [backends.md](../backends.md). Do not fail the shell when another notification daemon is already running. The UI receives the same `NotificationEvent` in all three cases.

### D-Bus server

Use `zbus` with `#[interface(name = "org.freedesktop.Notifications")]` on an object at `/org/freedesktop/Notifications`. Request the name `org.freedesktop.Notifications` on the session bus. If another daemon owns the name, do not start a second daemon. Resolve to the proxy backend and read the existing daemon through its D-Bus API.

Do not use a nested main loop. Await the bus name request and object registration inside the Tokio runtime.

### Types

```rust
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub hints: HashMap<String, zvariant::OwnedValue>,
    pub expire_timeout: i32, // milliseconds, -1 means server default
    pub time: i64,           // Unix seconds
    pub state: NotificationState,
}

pub struct NotificationAction {
    pub id: String,
    pub label: String,
}

pub enum ClosedReason {
    Expired = 1,
    DismissedByUser = 2,
    Closed = 3,
    Undefined = 4,
}
```

Add helpers for the standard hints:

- `image-path`, `image-data`.
- `action-icons`.
- `category`.
- `desktop-entry`.
- `resident`.
- `sound-file`, `sound-name`, `suppress-sound`.
- `transient`.
- `x`, `y`.
- `urgency` (`0` low, `1` normal, `2` critical).
- `body-images`, `body-hyperlinks`.
- `activation-token`.

Preserve unknown hints. Do not drop them on persistence.

### Notify

1. Sanitize the app name, app icon, summary, and body. Keep the body as markup; do not strip tags.
2. Normalize `image-data` and `image_data` to a cached file path. Remove `icon_data`.
3. Resolve the timeout: `expire_timeout >= 0 ? expire_timeout : default_timeout`. A negative default means no timeout.
4. Determine `replaced = store.contains_key(replaces_id)`.
5. Reuse `replaces_id` when replaced, otherwise allocate the next ID.
6. Build actions from pairs. Ignore an odd trailing string.
7. Insert the notification with `state = Received`.
8. Emit `Notified { id, replaced }` to the UI.
9. Schedule the daemon timeout if `!ignore_timeout && timeout > 0`.
10. Schedule a persistence flush in 100 ms.
11. Return the ID.

If `dont_disturb` is true, still store and emit `Notified`, but mark the event `suppress_popup = true`. The UI decides not to create a card. The daemon timeout still applies.

### Close and actions

`CloseNotification(id)` resolves with `Closed`.

`Dismiss(id)` from the UI resolves with `DismissedByUser`. `Expire(id)` resolves with `Expired`.

When a UI action is invoked, call the same path as `ActionInvoked`:

- Emit the D-Bus `ActionInvoked(id, action_id)` signal.
- If the notification is not resident, resolve with `Closed`.

Do not remove a resident notification when an action is invoked.

### Capabilities

Return the same list as the current GSettings default:

```text
action-icons, actions, body, body-hyperlinks, body-images,
body-markup, icon-static, persistence, sound
```

Add `body-images` only if the UI can render them. Add `persistence` because notifications survive a restart.

### Persistence

Use `~/.local/state/nosh/notifications.json`. Write a JSON object with:

```json
{
  "next_id": 12,
  "notifications": [
    {
      "id": 1,
      "app_name": "...",
      "app_icon": "...",
      "summary": "...",
      "body": "...",
      "actions": [{ "id": "...", "label": "..." }],
      "hints": {},
      "expire_timeout": 5000,
      "time": 1700000000,
      "transient": false
    }
  ]
}
```

Persist only non-transient notifications. Write atomically with a temporary file and rename. Debounce writes by 100 ms. Restore on startup and set `next_id` above the highest restored ID.

Do not use Astal's GSettings schema. The rewrite owns its schema, for example `com.nicolaegr.nosh` with a `notifications` path. Keep notification payloads out of GSettings; use the JSON state file above for the notification list. GSettings is for settings that need typed keys, live change notification, and a `gsettings` CLI.

On first run, if the old `io.astal.notifd` schema exists, import `ignore-timeout`, `dont-disturb`, and `default-timeout` once. Write a marker in the new schema so the import does not repeat. Do not depend on Astal at runtime.

### Settings

Use own GSettings keys:

```text
notifications ignore-timeout    b   false
notifications dont-disturb      b   false
notifications default-timeout   i   -1
notifications max-visible       i   3
notifications display-timeout   i   5000
notifications position          s   "top-right"
notifications monitor-policy    s   "farthest"
notifications server-capabilities as [...]
```

What each setting does:

- `ignore-timeout`: ignore the sender's `expire_timeout` and never auto-expire.
- `dont-disturb`: store notifications but suppress popup cards. Exposed to the quick settings DND toggle.
- `default-timeout`: timeout for notifications that send `-1`, in milliseconds. Negative means no timeout.
- `max-visible`: maximum number of cards on screen. Current behavior is 3.
- `display-timeout`: UI card lifetime in milliseconds. Current behavior is 5000. It is separate from the daemon timeout.
- `position`: `top-left`, `top-right`, `bottom-left`, or `bottom-right`.
- `monitor-policy`: `farthest`, `primary`, `current`, or `named:<connector>`.
- `server-capabilities`: the list returned by `GetCapabilities`.

Changing `dont-disturb` emits `NotificationEvent::DndChanged`. Changing `position` or `monitor-policy` moves the notification window without recreating cards. See `features/monitors.md` for the selection algorithm.

### Image cache

Use `~/.cache/nosh/notifications/`. Hash the image bytes with SHA-256, write `<hash>.png`. Cap width, height, and stride. Reject unsupported bit depths and color spaces with a debug log. Do not convert every hint into a file.

### Icon lookup

Do the icon lookup in the UI, because GTK owns the icon theme. The core stores `app_icon` and `image_path`. The UI resolves the themed icon.

## UI

### Placement

Notifications always appear on the farthest monitor in the configured corner direction. This is not the current monitor and not every monitor. The rule is in `features/monitors.md`.

- Read `notifications position` and `notifications monitor-policy` from GSettings.
- Compute the target monitor from the current `MonitorLayout`.
- Set `gtk4_layer_shell::set_monitor` to that monitor before presenting.
- Anchor to the requested corner with the current margins: 8 px on the top or bottom edge and 28 px on the left or right edge.
- Recompute on monitor layout changes, resolution changes, and setting changes. Move the window; do not recreate cards.

Example: `position = "top-right"` places cards at the top-right corner of the top-right-most monitor. If the rightmost monitor is removed, the window moves to the next monitor that matches the same corner rule.

### Window

Create a `gtk::Window` with `gtk4-layer-shell`:

- Anchor according to `position`.
- Layer `Overlay`.
- Exclusivity `Ignore`.
- Keyboard mode `None`.
- Namespace `nosh-notifications-outer`.
- Margin 8 px on the vertical edge and 28 px on the horizontal edge.
- Monitor from the placement rule.

Class `Notifications-outer`. Do not add the 64 px CSS margin on top of the layer margin. Pick one. If the cards must clear the bar, use a top margin of 32 or 64 in layer-shell and remove the CSS margin.

The window is hidden when there are no visible cards. Hide it before moving it to another monitor, then present it again so the layer surface is recreated on the right output.

### Container

- Vertical `gtk::Box`, class `NotificationsContainer`, spacing 4.
- Min width 360, max width 400.
- A `VecDeque` for waiting cards.
- `MAX_VISIBLE = 3`.
- On `NotificationReceived`:
  - If `suppress_popup` or DND, do not create a card. The notification stays in the store.
  - If fewer than 3 visible cards, create and append.
  - Else push to the queue.
- On card dismissal, remove it, show the next queued card, and hide the window when empty.
- Add `pointer-events-none` when empty so the window does not capture clicks.

Use a keyed map from notification ID to card. A replacement updates the existing card instead of adding a second one.

### Card

Build the same structure. Use classes:

```text
NotificationCard p-2 gap-1 rounded-2xl
```

Header classes `gap-2 header`. Title and time `heading`. Buttons `min-w-0 min-h-0 square-icon`. Body label uses markup, wrap `WORD_CHAR`, max 40 chars, 3 lines. Actions row `gap-1 mt-2`, homogeneous.

Timers:

- Default display timeout 5000 ms.
- Pause on enter, resume on leave with the remaining time.
- Use `glib::timeout_add_local` or a `glib::Source` with a monotonic deadline.
- On timeout, call `Command::NotificationExpire(id)` and animate out. The close reason is `Expired`, and the UI keeps the notification for the centre panel list.
- On close click, call `Command::NotificationDismiss(id)` and animate out. The close reason is `DismissedByUser`, and the notification is not kept.
- On action click, call `Command::NotificationInvoke { id, action }`.

Do not call D-Bus directly.

### Urgency and DND

Optional for v1:

- Add a `critical` class for urgency 2 and keep the card visible until dismissed.
- Add a `low` class for urgency 0 and use a shorter timeout.
- Do not play sounds. The sound hints are stored but ignored.

DND is required: when `dont_disturb` is true, no cards appear. The notification is still stored and appears in a future notification center.

### Animations

Keep the CSS keyframes `notification-bounce-in` and `notification-slide-out`. Use `add_css_class("notification-card-appear")` on creation and `add_css_class("notification-card-dismiss")` before removal. Do not rebuild the stack during the animation.

## Defects to fix

- No nested main loop at startup.
- No Astal GSettings dependency at runtime. The shell uses its own schema.
- The display timeout and daemon timeout are separate and documented.
- DND suppresses popups.
- A replacement updates the existing card.
- The image cache rejects unsupported data and caps sizes.
- The UI does not block on D-Bus.
- The container does not capture clicks when empty.
- The `Notifications-outer` margin does not double up with the layer margin.

## Definition of done

- `notify-send` shows a card in the top-right.
- The card shows the app icon, title, time, body, and actions.
- Clicking an action emits `ActionInvoked` and resolves non-resident notifications.
- Clicking close resolves with `DismissedByUser`.
- The card timer pauses on hover and resumes with the remaining time.
- At most 3 cards are visible; the rest queue.
- A replacement updates the same card.
- DND suppresses cards but stores notifications.
- Notifications appear on the farthest monitor for the configured corner, not on every monitor.
- Moving the notification window after a monitor change does not lose or recreate cards.
- Notifications survive a restart when not transient.
- The daemon owns `org.freedesktop.Notifications` and reports the current server information and capabilities.
- `ignore-timeout` and `default-timeout` behave as specified.
- No GTK code is in the daemon and no D-Bus code is in the card.
