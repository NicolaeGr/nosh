namespace TopBar.Widgets {
    public class Workspaces : Gtk.Box {
        AstalHyprland.Hyprland hypr = AstalHyprland.get_default ();
        private HashTable<int, ulong> workspace_handlers = new HashTable<int, ulong> (direct_hash, direct_equal);
        private HashTable<int, Gtk.Button> workspace_buttons = new HashTable<int, Gtk.Button> (direct_hash, direct_equal);
        private HashTable<int, bool> urgent_workspaces = new HashTable<int, bool> (direct_hash, direct_equal);

        public Workspaces () {
            set_css_classes ({"Workspaces", "p-1", "mr-2", "gap-1"});
            
            set_spacing (0);
            hypr.notify["workspaces"].connect (sync);
            hypr.urgent.connect (on_urgent);
            sync ();
        }

        void sync () {
            var handler_ids = workspace_handlers.get_values ();
            foreach (ulong handler_id in handler_ids) {
                hypr.disconnect (handler_id);
            }
            workspace_handlers.remove_all ();
            workspace_buttons.remove_all ();
            
            clear_children (this);

            var ordered = hypr.workspaces;
            ordered.sort ((a, b) => a.id - b.id);

            foreach (var ws in ordered) {
                if (ws.id < -99 || ws.id > -2)
                    append (create_button (ws));
            }
        }

        void on_urgent (AstalHyprland.Client client) {
            if (client.workspace == null)
                return;

            var ws_id = client.workspace.id;

            // Mark workspace as urgent if it's not the focused one
            if (hypr.focused_workspace.id != ws_id) {
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

            btn.set_css_classes({"min-w-8", "min-h-2","p-0","mx-0", "text-xs", "font-semibold", "opacity-60", "transition-all", "duration-200", "ease-in-out"});

            // Store button for later reference
            workspace_buttons.insert (ws.id, btn);

            // Check if this workspace is urgent
            if (urgent_workspaces.lookup (ws.id)) {
                btn.add_css_class ("urgent");
            }

            // Set focused state
            if (hypr.focused_workspace == ws)
                btn.add_css_class ("focused");

            var handler_id = hypr.notify["focused-workspace"].connect (() => {
                if (hypr.focused_workspace == ws) {
                    btn.add_css_class ("focused");
                    // Clear urgent state when workspace becomes focused
                    btn.remove_css_class ("urgent");
                    urgent_workspaces.remove (ws.id);
                } else {
                    btn.remove_css_class ("focused");
                }
            });
            workspace_handlers.insert (ws.id, handler_id);

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
