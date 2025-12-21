namespace Widgets {
    public class Workspaces : Gtk.Box {
        AstalHyprland.Hyprland hypr = AstalHyprland.get_default ();

        public Workspaces () {
            add_css_class ("Workspaces");
            hypr.notify["workspaces"].connect (sync);
            sync ();
        }

        void sync () {
            clear_children (this);

            var ordered = hypr.workspaces;
            ordered.sort ((a, b) => a.id - b.id);

            foreach (var ws in ordered) {
                if (ws.id < -99 || ws.id > -2)
                    append (create_button (ws));
            }
        }

        private Gtk.Button create_button (AstalHyprland.Workspace ws) {
            var btn = new Gtk.Button () {
                label = ws.id.to_string (),
                height_request = 24,
                width_request = 24,
            };

            // initial state
            if (hypr.focused_workspace == ws)
                btn.add_css_class ("focused");

            // update on change
            hypr.notify["focused-workspace"].connect (() => {
                if (hypr.focused_workspace == ws)
                    btn.add_css_class ("focused");
                else
                    btn.remove_css_class ("focused");
            });

            btn.clicked.connect (ws.focus);
            return btn;
        }
    }
}