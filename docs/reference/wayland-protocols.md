# Wayland protocols

Pseudocode summaries of the Wayland protocols the shell uses. Protocol names, request names, event names, and argument types are functional facts. Verify the installed protocol XML and compositor support.

## Registry

```text
display = connect(WAYLAND_DISPLAY)
registry = display.get_registry()

on global(name, interface, version):
  store global

bind(interface, name, min(version, supported_version))
```

Never assume a global exists. Probe the registry and fall back through the try tree.

## wl_output

```text
interface wl_output version 4
  event geometry(x, y, physical_width, physical_height, subpixel, make, model, transform)
  event mode(flags, width, height, refresh)
  event done()
  event scale(factor)
  event name(name)
  event description(description)
```

`flags & 1` marks the current mode. Use the current mode for logical size.

`wl_output` geometry is in physical pixels. Apply scale and transform to get logical geometry. If `zxdg_output_manager_v1` is available, prefer its logical position and size.

## zxdg_output_manager_v1

```text
interface zxdg_output_manager_v1
  request get_xdg_output(output) -> zxdg_output_v1
```

```text
interface zxdg_output_v1
  event logical_position(x, y)
  event logical_size(width, height)
  event done()
  event name(name)          deprecated in newer versions
  event description(text)   deprecated in newer versions
```

Use logical position and size when present. The compositor sends `wl_output.done` for the final commit on newer versions.

## ext-workspace-v1

Manager:

```text
interface ext_workspace_manager_v1
  request commit()
  request stop()

  event workspace_group(group: ext_workspace_group_handle_v1)
  event workspace(workspace: ext_workspace_handle_v1)
  event done()
  event finished()
```

Group:

```text
interface ext_workspace_group_handle_v1
  request create_workspace(name)
  request destroy()

  event capabilities(capabilities)
  event output_enter(output: wl_output)
  event output_leave(output: wl_output)
  event workspace_enter(workspace: ext_workspace_handle_v1)
  event workspace_leave(workspace: ext_workspace_handle_v1)
  event removed()
```

Workspace:

```text
interface ext_workspace_handle_v1
  request activate()
  request deactivate()
  request assign(group: ext_workspace_group_handle_v1)
  request remove()
  request destroy()

  event id(id)
  event name(name)
  event coordinates(coordinates)
  event state(state)
  event capabilities(capabilities)
  event removed()
```

Enums:

```text
group capabilities:
  CREATE_WORKSPACE

workspace state:
  ACTIVE
  URGENT
  HIDDEN

workspace capabilities:
  ACTIVATE
  DEACTIVATE
  REMOVE
  ASSIGN
```

Pseudocode for applying pending state:

```text
on workspace_group: add group to pending
on workspace: add workspace to pending
on done:
  apply created groups
  apply created workspaces
  apply workspace state and capabilities
  remove deleted workspaces
  remove deleted groups
  emit snapshot or items_changed
on finished:
  stop updates and report that the compositor will not send more
```

Do not read a workspace handle before `done` commits its initial state. Do not call `activate` unless the workspace has `ACTIVATE`.

Special workspaces are not marked by the protocol. Hide a workspace whose name starts with `special:`.

## zwp_idle_inhibit_manager_v1

```text
interface zwp_idle_inhibit_manager_v1
  request destroy()
  request create_inhibitor(surface: wl_surface) -> zwp_idle_inhibitor_v1
```

```text
interface zwp_idle_inhibitor_v1
  request destroy()
```

Pseudocode:

```text
create one wl_surface at startup
commit it once
on enable:
  if inhibitor is null:
    inhibitor = manager.create_inhibitor(surface)
    surface.commit()
    flush()
on disable:
  if inhibitor is not null:
    inhibitor.destroy()
    inhibitor = null
    surface.commit()
    flush()
```

The inhibitor is tied to the surface lifetime. Keep the surface alive while the inhibitor is active. Do not roundtrip on every toggle.

If the global is missing, report unavailable. Do not claim the inhibitor is active.

## ext-idle-notify-v1

This is the opposite of the inhibitor. It reports idle state. It is a future feature.

```text
interface ext_idle_notifier_v1
  request destroy()
  request get_idle_notification(timeout: u, seat: wl_seat) -> ext_idle_notification_v1
  request get_input_idle_notification(timeout: u, seat: wl_seat) -> ext_idle_notification_v1
```

```text
interface ext_idle_notification_v1
  request destroy()
  event idled()
  event resumed()
```

`get_input_idle_notification` ignores idle inhibitors. `get_idle_notification` respects them.

Pseudocode:

```text
notification = notifier.get_input_idle_notification(600000, seat)
on idled: mark idle
on resumed: mark active
```

Do not confuse this with the inhibitor. Use the inhibitor for No Sleep. Use this only for lock, screen blank, or hide-when-idle behavior.

## zwlr_output_manager_v1

wlroots output configuration protocol. Not every compositor supports it.

```text
interface zwlr_output_manager_v1
  request create_configuration() -> zwlr_output_configuration_v1
  request stop()
  event head(head: zwlr_output_head_v1)
  event done(serial: u)
  event finished()
```

Head:

```text
interface zwlr_output_head_v1
  request release()
  event name(name)
  event description(description)
  event physical_size(width, height)
  event mode(mode: zwlr_output_mode_v1)
  event enabled(enabled)
  event current_mode(mode)
  event position(x, y)
  event transform(transform)
  event scale(scale)
  event finished()
  event make(make)
  event model(model)
  event serial_number(serial)
  event adaptive_sync(state)
```

Mode:

```text
interface zwlr_output_mode_v1
  request release()
  event size(width, height)
  event refresh(refresh)
  event preferred()
  event finished()
```

Configuration:

```text
interface zwlr_output_configuration_v1
  request enable_head(head) -> zwlr_output_configuration_head_v1
  request disable_head(head)
  request apply()
  request test()
  request destroy()
  event succeeded()
  event failed()
  event cancelled()
```

Configuration head:

```text
interface zwlr_output_configuration_head_v1
  request set_mode(mode)
  request set_custom_mode(width, height, refresh)
  request set_position(x, y)
  request set_transform(transform)
  request set_scale(scale)
  request set_adaptive_sync(state)
```

Pseudocode for apply:

```text
wait for done(serial)
configuration = manager.create_configuration()
for monitor in desired_layout:
  if monitor.enabled:
    head_config = configuration.enable_head(head)
    set mode, position, transform, scale
  else:
    configuration.disable_head(head)
configuration.apply()
wait for succeeded or failed
```

Do not apply a configuration until the manager has sent `done`. Treat `failed` and `cancelled` as normal outcomes.

## Capability probing

```text
registry.find("ext_workspace_manager_v1") -> generic workspaces
registry.find("zwp_idle_inhibit_manager_v1") -> idle inhibitor
registry.find("ext_idle_notifier_v1") -> idle detection
registry.find("zwlr_output_manager_v1") -> generic output write
registry.find("zxdg_output_manager_v1") -> logical output geometry
```

A global presence is not a guarantee that the compositor implements every request correctly. Probe with a harmless read and handle protocol errors.

## Testing

Use a fake registry and fake global objects. Test:

- A missing global produces an unavailable result.
- Pending ext-workspace state is not visible before `done`.
- A workspace without `ACTIVATE` is not activated.
- Special workspace names are hidden.
- The inhibitor creates and destroys on toggle.
- A failed output configuration does not change the last known layout.
- Protocol version negotiation uses the minimum supported version.
