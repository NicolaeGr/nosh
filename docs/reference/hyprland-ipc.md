# Hyprland IPC

Pseudocode summary of the Hyprland runtime interface used by the shell. Verify against the installed Hyprland version. Hyprland changes event payloads and config syntax between releases.

## Environment

```text
HYPRLAND_INSTANCE_SIGNATURE   instance signature
XDG_RUNTIME_DIR               runtime directory
command socket                $XDG_RUNTIME_DIR/hypr/$SIGNATURE/.socket.sock
event socket                  $XDG_RUNTIME_DIR/hypr/$SIGNATURE/.socket2.sock
```

If `HYPRLAND_INSTANCE_SIGNATURE` is missing, Hyprland is not the running compositor. Do not guess.

## Command socket

Request and response framing:

```text
connect to command socket
write request bytes
read until byte 0x04
response = bytes before 0x04 as UTF-8
```

The socket is request and response. Serialize requests through one task. Do not run concurrent requests on the same connection without framing.

## Queries

```text
j/status             JSON object with config provider and version-related fields
j/version            JSON object with tag, commit, branch, flags
j/monitors           JSON array of monitors
j/workspaces         JSON array of workspaces
j/clients            JSON array of clients
j/activeworkspace    JSON object for the focused workspace
j/activewindow       JSON object for the focused client
j/binds              JSON array of keybind entries
cursorpos            text "x,y"
```

## Config provider and version

```text
status = json(command("j/status"))
provider = status.configProvider if present
version = json(command("j/version"))

if provider == "lua":
  use Lua dispatch syntax
else if provider == "hyprlang":
  use hyprlang dispatch syntax
else:
  use a version table as a fallback
  re-check after configreloaded and after reconnect
```

Do not assume the provider from `j/version` alone. A user can switch config language without changing the Hyprland binary. Use the provider when present, then the version as a fallback.

## Event socket

Each line:

```text
event_name ">>" payload
```

Examples:

```text
workspacev2>>1,1
focusedmon>>DP-1,1
activewindowv2>>55a1b2c3
urgent>>55a1b2c3
openwindow>>55a1b2c3,1,kitty,Terminal
closewindow>>55a1b2c3
```

Some events have no payload. Split once at `>>` and validate the payload before indexing.

## Events used

| Event | Payload | Action |
| --- | --- | --- |
| `workspacev2` | `id,name` | Refresh workspaces and monitors; set focused workspace |
| `focusedmon` | `monitor,workspace` | Refresh monitors; set focused monitor and workspace |
| `createworkspacev2` | `id,name` | Add workspace; refresh |
| `destroyworkspacev2` | `id,name` | Remove workspace; refresh |
| `moveworkspacev2` | `id,name,monitor` | Refresh workspaces and monitors |
| `renameworkspace` | `id,new_name` | Refresh workspaces |
| `activespecial` | `name,monitor` | Refresh monitors and workspaces; keep special workspaces hidden |
| `openwindow` | `address,workspace,class,title` | Refresh clients if needed |
| `closewindow` | `address` | Remove client; refresh workspaces |
| `movewindowv2` | `address,workspace,monitor` | Refresh clients and workspaces |
| `urgent` | `address` | Mark the client's workspace urgent if not focused |
| `activewindowv2` | `address` | Set focused client |
| `changefloatingmode` | `address,floating` | Refresh clients |
| `fullscreen` | `fullscreen` | Refresh clients |
| `windowtitlev2` | `address,title` | Refresh clients |
| `configreloaded` | none | Re-read provider and version |
| `monitoraddedv2` | `id,name,description` | Refresh monitors and workspaces |
| `monitorremoved` | `name` | Refresh monitors and workspaces |
| `submap` | `name` | Optional UI state |
| `activelayout` | `keyboard,layout` | Optional UI state |

Ignore group events for now. If group support is added, parse `togglegroup`, `moveintogroup`, `moveoutofgroup`, `ignoregrouplock`, and `lockgroups` without crashing on unknown fields.

## Monitor JSON

Fields used:

```text
id              int
name            string, e.g. "DP-1"
description     string
make            string
model           string
serial          string
width           int, physical pixels
height          int
refreshRate     double
x               int, layout position
y               int
scale           double
transform       int, 0..7
focused         bool
dpmsStatus      bool
vrr             bool
activelyTearing bool
disabled        bool
currentFormat   string
availableModes  array<string>
reserved        [top, bottom, left, right]
activeWorkspace object with id, name
specialWorkspace object with id, name
mirrorOf        string, optional, newer versions
```

## Workspace JSON

Fields used:

```text
id              int
name            string
monitorID       int
monitor         string, optional
hasfullscreen   bool
lastwindow      string
windows         int
```

Normal workspaces have positive ids. Special workspaces have negative ids and names starting with `special:`. Do not show special workspaces.

## Client JSON

Fields used:

```text
address          string, "0x..." or without prefix
mapped           bool
hidden           bool
at               [x, y]
size             [width, height]
workspace        object with id, name
floating         bool
monitor          int
class            string
title            string
initialClass     string
initialTitle     string
pid              uint
xwayland         bool
pinned           bool
fullscreen       int
fullscreenClient int
inhibitingIdle   bool, newer versions
grouped          array<string>, optional
swallowing       string
focusHistoryID   int
```

Normalize addresses by removing a leading `0x` and keeping one canonical form.

## Dispatch

Hyprlang:

```text
dispatch workspace <id>
dispatch focuswindow address:0x<address>
dispatch focusmonitor <id>
dispatch closewindow address:0x<address>
dispatch movetoworkspacesilent <workspace>,address:0x<address>
dispatch togglefloating address:0x<address>
dispatch moveworkspacetomonitor <workspace> <monitor>
dispatch movecursor <x> <y>
dispatch exec <command>
```

Lua:

```text
dispatch hl.dsp.focus({ workspace = "<id>" })
dispatch hl.dsp.focus({ window = "address:0x<address>" })
dispatch hl.dsp.focus({ monitor = <id> })
dispatch hl.dsp.window.close({ window = "address:0x<address>" })
dispatch hl.dsp.window.move({ window = "address:0x<address>", workspace = <id>, follow = false })
dispatch hl.dsp.window.float({ window = "address:0x<address>" })
dispatch hl.dsp.workspace.move({ workspace = <id>, monitor = <monitor> })
dispatch hl.dsp.cursor.move({ x = <x>, y = <y> })
dispatch hl.dsp.exec_cmd("<command>")
```

The Lua table syntax is sensitive. Keep the formatter in one module with tests. Do not build dispatch strings in the UI.

## Monitor configuration

Hyprlang runtime path:

```text
hyprctl keyword monitor "<name>,<mode>,<position>,<scale>"
hyprctl keyword monitor "<name>,<mode>,<position>,<scale>,mirror,<source>"
hyprctl keyword monitor "<name>,disable"
```

Examples:

```text
hyprctl keyword monitor "DP-2,2560x1440@144,0x0,1.0"
hyprctl keyword monitor "DP-2,2560x1440@144,0x0,1.0,mirror,DP-1"
hyprctl keyword monitor "DP-2,disable"
```

Lua provider:

```text
the runtime keyword path may be rejected when the config provider is Lua
check the installed version and its runtime output API
if no Lua runtime call exists, write the Lua config and reload
do not pretend the write succeeded
```

Persist layout only when the user asks. Runtime changes are the default. Back up the compositor config before writing it.

## Read loop pseudocode

```text
connect_event_socket:
  loop:
    open event socket
    if open fails:
      emit Unavailable
      sleep with backoff
      continue
    stream = buffered reader
    emit Connected
    refresh_state()
    while true:
      line = read_line()
      if line is null:
        break
      split line at ">>"
      handle_event(name, payload)
    close socket
    sleep with backoff
```

Refresh state:

```text
provider = read_provider()
version = read_version()
monitors = json("j/monitors")
workspaces = json("j/workspaces")
clients = json("j/clients")
focused_workspace = json("j/activeworkspace")
focused_client = json("j/activewindow")
filter special workspaces
emit Snapshot
```

## Error handling

- Missing socket: unavailable, retry.
- EOF: stop reading, reconnect.
- Unknown event: ignore and log at debug level once.
- Missing payload field: ignore the event and log at debug level.
- Missing object referenced by an event: no-op, not a critical.
- Non-`ok` dispatch response: log once per failure streak.
- Do not spin on `read_line` after EOF.
