# Workspaces

Current files: `src/bar/widgets/Workspaces.vala`, `src/bar/Left.vala`, `src/bar/Bar.scss`.

This is the only feature that uses Hyprland state directly in the UI. The rewrite moves all of it into `nosh-core`.

## Current behavior

`Workspaces` is a `Gtk.Box` with CSS classes `Workspaces`, `p-1`, `mr-2`, `gap-1` and spacing 0.

On construction it gets `AstalHyprland.get_default()`. If the singleton is null, it retries every 500 ms. When available, it connects:

- `notify["workspaces"]` to `sync`.
- `urgent` to `on_urgent`.

`sync` disconnects per-workspace handlers, clears all buttons, reads `hypr.workspaces`, sorts by `id`, and appends a button for each workspace that passes:

```text
ws.id < -99 || ws.id > -2
```

Each button:

- Label is the workspace ID as a string.
- CSS classes `min-w-8`, `min-h-2`, `p-0`, `mx-0`, `text-xs`, `font-semibold`, `opacity-60`, `transition-all`, `duration-200`, `ease-in-out`.
- CSS class `focused` when `hypr.focused_workspace == ws`.
- CSS class `urgent` when the workspace is in the urgent table.
- On focus change, adds `focused` and clears `urgent` for the newly focused workspace, and removes `focused` from others.
- On click, sends `dispatch hl.dsp.focus({ workspace = "<id>" })` through `hypr.message_async`, then clears urgent.
- The intended `ws.focus()` call is commented out with `TODO: Fix this when Hyprland API is updated`.

Urgent handling: when `hypr.urgent(client)` fires, if the client has a workspace and the workspace is not focused, mark it urgent and add the class to its button.

The current filter includes `-1` and `0` and excludes `-99..-2`. Hyprland uses negative IDs for special workspaces. The rewrite does not preserve this predicate; it never shows special workspaces.

## Core

### Backend choice

Use the try tree from [backends.md](../backends.md), specific first:

| Order | Backend | Probe | Capabilities |
| --- | --- | --- | --- |
| 1 | Hyprland IPC | `HYPRLAND_INSTANCE_SIGNATURE`, working `hyprctl -j version` | read, watch, activate, move, urgent, special |
| 2 | Other compositor adapter | compositor-specific socket or IPC | read, watch, activate, move when available |
| 3 | `ext-workspace-v1` | registry global | read, watch, activate, move when the handle advertises it |
| 4 | Static | none | empty list |

Define a `WorkspaceBackend` trait and one implementation per candidate. Hyprland comes first even when `ext-workspace-v1` is available because the Hyprland backend has exact urgent semantics, special workspace knowledge, and the current behavior users already see. The generic backend is the fallback and the path for compositors without a specific adapter.

Resolve each capability separately. A compositor adapter may support reads and activation but not moving a workspace to another monitor. In that case use the specific backend for reads and the generic backend for the move if it supports `ASSIGN`.

Do not scatter `if compositor == ...` in the UI. The UI receives `WorkspaceEvent` and calls `WorkspaceActivate`. The resolver owns the choice.

### Model

```rust
pub struct WorkspaceModel {
    pub id: String,              // Hyprland numeric id as string, or generic id/name
    pub name: String,
    pub monitor: Option<String>, // connector name when known
    pub special: bool,
    pub has_fullscreen: bool,
    pub focused: bool,
    pub urgent: bool,
    pub capabilities: WorkspaceCapabilities,
}

pub struct MonitorModel {
    pub id: i32,
    pub connector: String,
    pub focused: bool,
}
```

The core owns the authoritative workspace list, the focused ID, and the urgent set. The UI never derives focus or urgency from object identity.

Use a stable string key. For Hyprland, the key is the numeric id as a decimal string. For ext-workspace, prefer the workspace `id` and fall back to `name`. The key must survive a workspace object being replaced during a resync.

### Ext-workspace backend

Bind `ext_workspace_manager_v1` and listen for:

- `workspace_group` creation.
- `workspace` creation.
- `done`, which commits the pending state.
- `finished`, which means the compositor will send no more updates.

For each workspace group, track its outputs and its workspaces. For each workspace, track:

- `id`, `name`, `coordinates`.
- `state` flags: `ACTIVE`, `URGENT`, `HIDDEN`.
- `capabilities`: `ACTIVATE`, `DEACTIVATE`, `REMOVE`, `ASSIGN`.

Map groups to monitors by matching the group output to a `wl_output` and then to a connector name. If a group has multiple outputs, it belongs to each of them. A workspace can be in more than one group, so emit one model per monitor view rather than mutating a single list.

Call `activate()` for a workspace with the `ACTIVATE` capability. Call `commit()` after a batch of actions, or use `freeze_autocommit` and `thaw_autocommit` for a multi-step layout change.

Special workspaces: the protocol does not label them. Hide a workspace when its name starts with `special:` or when the compositor-specific backend marks it special. Do not show hidden workspaces in the bar.

### Hyprland backend

On connect, run these queries through the Hyprland command socket:

```text
j/status
j/version
j/monitors
j/workspaces
j/clients
j/activeworkspace
j/activewindow
```

`j/status` and `j/version` tell you the config provider and the running version. Use the provider first, then the version, to choose the dispatch syntax. Do not assume one syntax. A Hyprland upgrade can change the provider without changing the version string, and a downgrade can do the opposite.

`j/activeworkspace` gives focus. Do not use object equality.

Filter workspaces before emitting them:

- Show only normal workspaces. For Hyprland, this means `numeric_id > 0` and `name` not starting with `special:`.
- Never show a special workspace in the bar. The user explicitly does not want them.

Emit `WorkspaceEvent::Snapshot(Vec<WorkspaceModel>, Vec<MonitorModel>)`.

### Hyprland events

Subscribe to `socket2` and parse these event names:

| Event | Action |
| --- | --- |
| `workspacev2` | Resync workspaces and monitors, set focused workspace |
| `focusedmon` | Resync monitors, set focused monitor and workspace |
| `createworkspacev2` | Add workspace, then resync |
| `destroyworkspacev2` | Remove workspace, then resync |
| `moveworkspacev2` | Resync workspaces and monitors |
| `renameworkspace` | Resync workspaces |
| `activespecial` | Resync monitors and workspaces, hide special workspaces |
| `openwindow` | Refresh clients and workspace client counts if needed |
| `closewindow` | Remove client, refresh workspaces |
| `movewindowv2` | Refresh clients and workspaces |
| `urgent` | Mark the client's workspace urgent if it is not focused |
| `activewindowv2` | Set focused client |
| `changefloatingmode` | Refresh clients |
| `fullscreen` | Refresh clients |
| `windowtitlev2` | Refresh clients |
| `configreloaded` | Re-read the config provider and version |
| `monitoraddedv2`, `monitorremoved` | Refresh monitors and workspaces |

Ignore group events for now. Parse them if PR #428 lands, but do not expose UI.

Each event line is `"<name>>>><payload>"`. Some events have no payload. Validate the split before reading the payload.

### Urgent state

Keep `urgent: BTreeSet<String>` in the core.

- Generic backend: set urgent when the workspace state contains `URGENT`, clear it when the flag disappears.
- Hyprland backend: on `urgent(client)`, if the client's workspace is not focused, insert the workspace key. On focus of a workspace, remove its key. On workspace removal, remove its key.
- Emit `WorkspaceEvent::UrgentChanged { workspace_key, urgent }`.

The UI only renders the `urgent` CSS class from this set. It does not clear urgency itself.

### Commands

```rust
Command::WorkspaceActivate(String)
Command::WorkspaceMoveToMonitor { workspace: String, monitor: String }
```

For the generic backend, call `activate()` and `assign_to_group()` or the workspace's move capability. For Hyprland, format the dispatch according to the provider. See `astal-decisions.md` for the table. Send the command over the Hyprland command socket and read until the `\x04` terminator. Do not spawn `hyprctl` for every click. Log a non-`ok` reply once per failure, not per retry.

### Reconnect

The connection supervisor:

- Generic backend: reconnect when the registry global disappears or the Wayland connection dies. Rebind and emit a fresh snapshot.
- Hyprland backend: connect to `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`.
- If the environment variable is missing, emit `Unavailable` and retry every 2 seconds.
- If the socket is missing, emit `Unavailable` and retry with backoff.
- On EOF, stop the read loop and reconnect.
- After reconnect, re-read the provider and version, then emit a fresh snapshot.

The UI has no timers and no `get_default()` polling.

## UI

`Workspaces` is a `gtk::Box` with the same CSS classes and spacing 0. Each bar has its own instance. It stores a `BTreeMap<String, gtk::Button>` and updates it by key.

Render rules:

- Show only the workspaces for this bar's monitor. The core or the bar passes the monitor connector; filter by `WorkspaceModel.monitor`.
- Sort by numeric id when both are numeric, otherwise by name. Keep the order stable.
- Never show a special workspace. The core filters them; the UI also skips `workspace.special` as a guard.
- Button label is the workspace name for the generic backend, or the numeric id for Hyprland. The current shell uses the numeric id.
- Add `focused` when `workspace.focused`.
- Add `urgent` when `workspace.urgent`.
- On click, send `Command::WorkspaceActivate(workspace.id)`.
- Do not clear urgent locally. Wait for the core event.

Do not rebuild every button on every event. Add, remove, and update in place. This matters when the workspace list changes on every window open or close.

If the generic backend returns a workspace in multiple groups, the same workspace can appear on more than one bar. That is correct. Do not deduplicate across bars.

### CSS

Keep the current selectors. They are in `.Workspaces` and `.Workspaces button` in `src/bar/Bar.scss`:

```css
.Workspaces {
  background-color: rgba(255,255,255,0.08);
  border-top-left-radius: 0;
  border-bottom-right-radius: 0;
  border-top-right-radius: 14px;
  border-bottom-left-radius: 14px;
}
.Workspaces button {
  font-family: monospace;
  border: 2px solid transparent;
  border-top-left-radius: 0;
  border-bottom-right-radius: 0;
  border-top-right-radius: 10px;
  border-bottom-left-radius: 10px;
}
.Workspaces button:hover { border-bottom-color: @accent_bg_color; border-left-color: @accent_bg_color; transform: scale(1.05); }
.Workspaces button.focused { color: @accent_bg_color; background-color: alpha(@accent_bg_color, 0.15); border-bottom-color: @accent_bg_color; border-left-color: @accent_bg_color; }
.Workspaces button.urgent { border-right-color: @error_color; border-top-color: @error_color; }
```

The utility classes `min-w-8` (32 px), `min-h-2` (8 px), `text-xs` (12 px), `opacity-60`, `transition-all`, `duration-200`, `ease-in-out` stay.

## Defects to fix

- The UI retry timer and manual handler bookkeeping are gone.
- Focus comparison uses IDs, not object identity.
- The raw Lua string in the widget is gone. Dispatch is centralized in the core.
- Urgent state survives a workspace list resync.
- Null clients and missing workspaces are no-ops, not criticals.
- Special workspaces are never shown. This is now a fixed policy, not a choice.

## Definition of done

- Each monitor's bar shows only that monitor's workspaces.
- Workspace buttons appear in ascending numeric order.
- Clicking a button focuses the workspace through the selected backend, with the correct Hyprland dispatcher when the Hyprland backend is selected.
- The focused workspace gets the `focused` class and only one workspace per monitor has it.
- An urgent workspace gets the `urgent` class; focusing or clicking it clears the class.
- Special workspaces never appear, including the active special workspace.
- A Hyprland restart reconnects without a process restart and without a 500 ms UI timer.
- A missing event socket leaves the bar running and the workspaces widget empty.
- The try tree selects the Hyprland backend first, then another compositor-specific adapter, then `ext-workspace-v1`, then the empty backend.
- If the selected specific backend loses a capability, the resolver falls back for that capability without losing the rest of the feature.
- The selected backend appears in the state snapshot and the logs.
- The Hyprland backend detects the config provider and version before choosing dispatch syntax.
