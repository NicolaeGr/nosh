namespace AppLauncher.Widgets {
    public class SearchBar : Gtk.SearchEntry {
        public signal void search_changed(string query);
        public signal void activate_selected();
        public signal void move_next();
        public signal void move_previous();
        
        public SearchBar() {
            Object(
                placeholder_text: "Search applications...",
                hexpand: true
            );
            
            set_css_classes({"SearchBar"});
            
            changed.connect(() => {
                search_changed(get_text());
            });
            
            activate.connect(() => {
                activate_selected();
            });
            
            var key_controller = new Gtk.EventControllerKey();
            key_controller.key_pressed.connect(on_key_pressed);
            add_controller(key_controller);
        }
        
        private bool on_key_pressed(uint keyval, uint keycode, Gdk.ModifierType state) {
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
    }
}
