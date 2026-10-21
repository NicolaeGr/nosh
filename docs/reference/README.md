# Reference index

These files are original pseudocode summaries written for the Rust rewrite. They are not copied from Astal, NetworkManager, BlueZ, UPower, Hyprland, or any other implementation. Interface names, method names, property names, and protocol names are functional facts. The prose, ordering, examples, and pseudocode are original.

The goal is to avoid translating implementation source line by line. Before you copy actual code from any upstream project, read that project's license. Pseudocode is not a license bypass for copied implementation code.

## Files

- [D-Bus interfaces](dbus-interfaces.md): NetworkManager, BlueZ, UPower, PowerProfiles, MPRIS, notifications, StatusNotifier, DBusMenu, KDE Connect, logind.
- [Hyprland IPC](hyprland-ipc.md): sockets, JSON queries, event grammar, dispatch tables, monitor commands.
- [Wayland protocols](wayland-protocols.md): outputs, ext-workspace, idle inhibit, idle notify, output management.
- [GTK widgets](gtk-widgets.md): window trees, layer-shell setup, focus and key handling.
- [Build](build.md): Cargo layout, system dependencies, GSettings schema, PAM file, systemd unit, packaging.
- [Power menu](../features/power-menu.md): the first auth host UI.

## Attribution

Credit these projects and their contributors in the rewrite:

- Astal by Aylur and contributors. Used as the behavioral reference for the current shell.
- NetworkManager and libnm.
- BlueZ.
- UPower and power-profiles-daemon.
- The MPRIS specification and player implementations.
- The freedesktop.org notification specification.
- The StatusNotifierItem and DBusMenu specifications.
- KDE Connect.
- Hyprland and its IPC documentation.
- Wayland, wayland-protocols, and wlroots protocol documentation.
- GTK, gtk4-layer-shell, and the gtk-rs bindings.
- PipeWire, WirePlumber, PulseAudio, and the libpulse bindings.
- Linux-PAM.

Do not copy license headers, comments, or implementation bodies from those projects into the rewrite. Keep attribution in the project README or a credits file.

## How to use these files

1. Read the feature spec first.
2. Use the reference file for interface and protocol shapes.
3. Verify the installed version. D-Bus services and compositor protocols change.
4. Keep the pseudocode in the docs. Do not paste it into Rust as if it were a compiled API.
5. Add a test for every parsing or encoding rule that comes from a reference.
