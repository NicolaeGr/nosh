namespace AppLauncher.Widgets {
    [GtkTemplate (ui = "/com/nicolaegr/nosh/app_launcher/widgets/SearchBar.ui")]
    public class SearchBar : Gtk.Box {
        public signal void query_changed(string query);
        public signal void activate_selected();
        public signal void close_requested();
        public signal void arrow_up();
        public signal void arrow_down();
        public signal void arrow_left();
        public signal void arrow_right();
        public signal void exit_grid_mode();
        
        [GtkChild]
        private unowned Gtk.SearchEntry search_entry;
        [GtkChild]
        private unowned Gtk.Image plugin_icon;
        
        private bool grid_mode = false;
        
        public SearchBar() {
            Object();
        }
        
        construct {
            search_entry.changed.connect(() => {
                if (grid_mode) {
                    grid_mode = false;
                    exit_grid_mode();
                }
                query_changed(search_entry.get_text());
            });
            
            search_entry.activate.connect(() => {
                activate_selected();
            });
            
            var key_controller = new Gtk.EventControllerKey();
            key_controller.set_propagation_phase(Gtk.PropagationPhase.CAPTURE);
            key_controller.key_pressed.connect(on_key_pressed);
            search_entry.add_controller(key_controller);
        }
        
        public string get_text() {
            return search_entry.get_text();
        }
        
        public void set_text(string text) {
            search_entry.set_text(text);
        }
        
        private bool on_key_pressed(uint keyval, uint keycode, Gdk.ModifierType state) {
            if ((state & Gdk.ModifierType.CONTROL_MASK) != 0) {
                return false;
            }
            
            if (keyval == Gdk.Key.Escape) {
                if (grid_mode) {
                    grid_mode = false;
                    exit_grid_mode();
                    return true;
                }
                close_requested();
                return true;
            }
            
            if (grid_mode) {
                if (keyval == Gdk.Key.Up) {
                    arrow_up();
                    return true;
                }
                if (keyval == Gdk.Key.Down) {
                    arrow_down();
                    return true;
                }
                if (keyval == Gdk.Key.Left) {
                    arrow_left();
                    return true;
                }
                if (keyval == Gdk.Key.Right) {
                    arrow_right();
                    return true;
                }
            }
            
            if (keyval == Gdk.Key.Down) {
                grid_mode = true;
                arrow_down();
                return true;
            }
            
            return false;
        }
        
        public void set_plugin(Plugin? plugin) {
            if (plugin != null) {
                plugin_icon.set_from_icon_name(plugin.get_icon());
                plugin_icon.visible = true;
            } else {
                plugin_icon.visible = false;
            }
        }
        
        public void grab_focus() {
            search_entry.grab_focus();
        }
    }
}
