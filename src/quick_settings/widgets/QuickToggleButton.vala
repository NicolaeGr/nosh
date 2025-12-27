using Gtk;

namespace QuickSettings.Widgets {
    public class QuickToggleButton : Gtk.Button {
        private State.AppState app_state = State.AppState.get_instance ();
        public string toggle_type { get; set; }
        public bool active { get; set; default = false; }

        public QuickToggleButton (string type, string icon_name, string label_text) {
            Object ();
            toggle_type = type;

            var content = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            content.halign = Gtk.Align.CENTER;
            content.valign = Gtk.Align.CENTER;
            content.margin_top = 8;
            content.margin_bottom = 8;
            content.margin_start = 8;
            content.margin_end = 8;

            var icon = new Gtk.Image.from_icon_name (icon_name);
            icon.set_icon_size (Gtk.IconSize.LARGE);
            icon.set_css_classes ({"icon"});

            var label = new Gtk.Label (label_text);
            label.set_css_classes ({"QuickSettings-label"});
            label.set_wrap (true);
            label.set_justify (Gtk.Justification.CENTER);

            content.append (icon);
            content.append (label);
            set_child (content);

            set_css_classes ({"QuickSettings-toggle-button"});

            clicked.connect (() => {
                active = !active;
                update_state ();
            });

            notify["active"].connect (() => {
                update_state ();
            });
        }

        private void update_state () {
            if (active) {
                add_css_class ("active");
            } else {
                remove_css_class ("active");
            }
        }
    }
}
