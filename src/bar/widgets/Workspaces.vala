namespace TopBar.Widgets {
    public class Workspaces : Gtk.Box {
        private AstalHyprland.Hyprland hypr;
        private HashTable<int, ulong> workspace_handlers = new HashTable<int, ulong> (direct_hash, direct_equal);
        private HashTable<int, Gtk.Button> workspace_buttons = new HashTable<int, Gtk.Button> (direct_hash, direct_equal);
        private HashTable<int, bool> urgent_workspaces = new HashTable<int, bool> (direct_hash, direct_equal);
        private uint reconnect_timeout = 0;
        private ulong workspaces_handler = 0;
        private ulong urgent_handler = 0;

        public Workspaces () {
            set_css_classes ({ "Workspaces", "p-1", "mr-2", "gap-1" });
            set_spacing (0);

            init_hyprland ();

            // Cleanup on destroy
            destroy.connect (() => {
                if (reconnect_timeout > 0) {
                    GLib.Source.remove (reconnect_timeout);
                    reconnect_timeout = 0;
                }
                if (hypr != null) {
                    if (workspaces_handler > 0)hypr.disconnect (workspaces_handler);
                    if (urgent_handler > 0)hypr.disconnect (urgent_handler);
                }
            });
        }

        private void init_hyprland () {
            try {
                hypr = AstalHyprland.get_default ();

                if (hypr == null) {
                    schedule_reconnect ();
                    return;
                }

                // Disconnect old handlers
                if (workspaces_handler > 0)hypr.disconnect (workspaces_handler);
                if (urgent_handler > 0)hypr.disconnect (urgent_handler);

                workspaces_handler = hypr.notify["workspaces"].connect (sync);
                urgent_handler = hypr.urgent.connect (on_urgent);

                sync ();

                // Cancel any pending reconnect timer
                if (reconnect_timeout > 0) {
                    GLib.Source.remove (reconnect_timeout);
                    reconnect_timeout = 0;
                }
            } catch (Error e) {
                warning ("Failed to initialize Hyprland: %s", e.message);
                schedule_reconnect ();
            }
        }

        private void schedule_reconnect () {
            if (reconnect_timeout > 0)return;

            reconnect_timeout = GLib.Timeout.add (500, () => {
                init_hyprland ();
                reconnect_timeout = 0;
                return false;
            });
        }

        void sync () {
            if (hypr == null) {
                schedule_reconnect ();
                return;
            }

            var handler_ids = workspace_handlers.get_values ();
            foreach (ulong handler_id in handler_ids) {
                hypr.disconnect (handler_id);
            }
            workspace_handlers.remove_all ();
            workspace_buttons.remove_all ();

            clear_children (this);

            var ordered = hypr.workspaces;
            if (ordered == null || ordered.length () == 0) {
                return;
            }

            ordered.sort ((a, b) => a.id - b.id);

            foreach (var ws in ordered) {
                if (ws.id < -99 || ws.id > -2)
                    append (create_button (ws));
            }
        }

        void on_urgent (AstalHyprland.Client client) {
            if (hypr == null || client.workspace == null)
                return;


            var ws_id = client.workspace.id;

            // Mark workspace as urgent if it's not the focused one
            if (hypr.focused_workspace != null && hypr.focused_workspace.id != ws_id) {
                urgent_workspaces.insert (ws_id, true);
                var btn = workspace_buttons.lookup (ws_id);
                if (btn != null) {
                    btn.add_css_class ("urgent");
                }
            }
        }

        private Gtk.Button create_button (AstalHyprland.Workspace ws) {
            var btn = new Gtk.Button () {
                label = ws.id.to_string (),
                hexpand = false,
                vexpand = false,
            };

            btn.set_css_classes ({ "min-w-8", "min-h-2", "p-0", "mx-0", "text-xs", "font-semibold", "opacity-60", "transition-all", "duration-200", "ease-in-out" });

            // Store button for later reference
            workspace_buttons.insert (ws.id, btn);

            // Check if this workspace is urgent
            if (urgent_workspaces.lookup (ws.id)) {
                btn.add_css_class ("urgent");
            }

            // Set focused state
            if (hypr != null && hypr.focused_workspace != null && hypr.focused_workspace == ws)
                btn.add_css_class ("focused");

            if (hypr != null) {
                var handler_id = hypr.notify["focused-workspace"].connect (() => {
                    if (hypr != null && hypr.focused_workspace != null && hypr.focused_workspace == ws) {
                        btn.add_css_class ("focused");
                        // Clear urgent state when workspace becomes focused
                        btn.remove_css_class ("urgent");
                        urgent_workspaces.remove (ws.id);
                    } else {
                        btn.remove_css_class ("focused");
                    }
                });
                workspace_handlers.insert (ws.id, handler_id);
            }

            btn.clicked.connect (() => {
                ws.focus ();
                // Clear urgent state when clicking the workspace
                btn.remove_css_class ("urgent");
                urgent_workspaces.remove (ws.id);
            });

            return btn;
        }
    }
}