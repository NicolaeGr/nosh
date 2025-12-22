namespace TopBar {
    public class QuickSettings : Astal.Window {
        public QuickSettings () {
            Object (
                anchor: Astal.WindowAnchor.TOP
                | Astal.WindowAnchor.BOTTOM
                | Astal.WindowAnchor.LEFT
                | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.OVERLAY,
                keymode: Astal.Keymode.EXCLUSIVE,
                visible: false,
                namespace: "hypr-shell-quick-settings"
            );

            add_css_class ("QuickSettings");

            // Full screen background that closes on click
            var background = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            background.hexpand = true;
            background.vexpand = true;
            background.add_css_class ("QuickSettings-background");

            var content = new Gtk.Box (Gtk.Orientation.VERTICAL, 12);
            content.margin_top = 12;
            content.margin_end = 12;
            content.margin_start = 12;
            content.margin_bottom = 12;
            content.halign = Gtk.Align.END;
            content.valign = Gtk.Align.START;
            content.add_css_class ("QuickSettings-content");

            var title = new Gtk.Label ("Quick Settings");
            title.add_css_class ("QuickSettings-title");
            content.append (title);

            // Add some placeholder items
            var audio_item = new Gtk.Label ("Audio Controls");
            audio_item.add_css_class ("QuickSettings-item");
            content.append (audio_item);

            var display_item = new Gtk.Label ("Display Settings");
            display_item.add_css_class ("QuickSettings-item");
            content.append (display_item);

            var power_item = new Gtk.Label ("Power Settings");
            power_item.add_css_class ("QuickSettings-item");
            content.append (power_item);

            background.append (content);
            set_child (background);

            // Click on background (outside content) closes window
            var bg_click = new Gtk.GestureClick ();
            bg_click.pressed.connect (() => {
                this.visible = false;
            });
            background.add_controller (bg_click);

            // Click on content doesn't propagate to background
            var content_click = new Gtk.GestureClick ();
            content_click.pressed.connect (() => {
                // Stop propagation by returning early
            });
            content.add_controller (content_click);

            // Handle ESC to close
            var key_controller = new Gtk.EventControllerKey ();
            key_controller.key_pressed.connect ((keyval, keycode, state) => {
                if (keyval == 0xffc1 || keyval == 65307) {  // Escape key
                    this.visible = false;
                    return true;
                }
                return false;
            });
            ((Gtk.Widget) this).add_controller (key_controller);

            // Focus window when visible
            notify["visible"].connect (() => {
                if (visible) {
                    present ();
                    grab_focus ();
                }
            });
        }
    }
}
