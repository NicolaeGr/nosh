namespace AppLauncher.Widgets {
    public class SearchBar : Gtk.Box {
        public signal void query_changed(string query);
        public signal void activate_selected();
        public signal void move_next();
        public signal void move_previous();
        public signal void close_requested();
        public signal void move_down();
        
        private Gtk.SearchEntry search_entry;
        
        public SearchBar() {
            Object(
                orientation: Gtk.Orientation.HORIZONTAL
            );
            
            search_entry = new Gtk.SearchEntry();
            search_entry.placeholder_text = "Search applications...";
            search_entry.hexpand = true;
            search_entry.set_css_classes({"SearchBar"});
            
            append(search_entry);
            
            search_entry.changed.connect(() => {
                query_changed(search_entry.get_text());
            });
            
            search_entry.activate.connect(() => {
                activate_selected();
            });
            
            var key_controller = new Gtk.EventControllerKey();
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
            // Escape key
            if (keyval == Gdk.Key.Escape) {
                close_requested();
                return true;
            }
            
            // Down arrow - move to grid
            if (keyval == Gdk.Key.Down) {
                move_down();
                return true;
            }
            
            // Tab key
            if (keyval == Gdk.Key.Tab) {
                if ((state & Gdk.ModifierType.SHIFT_MASK) != 0) {
                    move_previous();
                } else {
                    move_next();
                }
                return true;
            }
            
            return false;
        }
        
        public void grab_focus() {
            search_entry.grab_focus();
        }
    }
}
