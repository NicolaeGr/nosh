namespace QuickSettings {
    public class Window : Astal.Window {
        private State.AppState app_state = State.AppState.get_instance ();
        private ulong visible_handler_id = 0;

        public Window () {
            Object (
                anchor: Astal.WindowAnchor.TOP
                | Astal.WindowAnchor.BOTTOM
                | Astal.WindowAnchor.LEFT
                | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.OVERLAY,
                keymode: Astal.Keymode.EXCLUSIVE,
                visible: false,
                namespace: "nosh-quick-settings"
            );

            set_css_classes ({"QuickSettings"});

            var background = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            background.hexpand = true;
            background.vexpand = true;
            background.set_css_classes ({"background"});

            var card = new Widgets.Card ();
            background.append (card);
            set_child (background);

            var bg_click = new Gtk.GestureClick ();
            bg_click.pressed.connect ((n_press, x, y) => {
                if (background.pick (x, y, Gtk.PickFlags.DEFAULT) == background) {
                    app_state.quick_settings_open = false;
                }
            });
            background.add_controller (bg_click);

            var key_controller = new Gtk.EventControllerKey ();
            key_controller.key_pressed.connect ((keyval, keycode, state) => {
                if (keyval == 0xffc1 || keyval == 65307) {  // Escape key
                    app_state.quick_settings_open = false;
                    return true;
                }
                return false;
            });
            ((Gtk.Widget) this).add_controller (key_controller);

            app_state.bind_property ("quick-settings-open", this, "visible", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);

            visible_handler_id = notify["visible"].connect (() => {
                if (visible) {
                    present ();
                    grab_focus ();
                }
            });
            
            close_request.connect (() => {
                if (visible_handler_id > 0) {
                    disconnect (visible_handler_id);
                }
                return false;
            });
        }
    }
}
