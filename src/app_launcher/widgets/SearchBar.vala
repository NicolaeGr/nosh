namespace AppLauncher.Widgets {
    public class SearchBar : Gtk.Box {
        public signal void query_changed(string query);
        public signal void activate_selected();
        public signal void close_requested();
        public signal void arrow_up();
        public signal void arrow_down();
        public signal void arrow_left();
        public signal void arrow_right();
        public signal void exit_grid_mode(); // Signal to reset grid selection
        
        private Gtk.SearchEntry search_entry;
        private bool grid_mode = false; // Track if we're navigating the grid
        
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
                // Any text change exits grid mode and resets selection
                if (grid_mode) {
                    grid_mode = false;
                    exit_grid_mode();
                }
                query_changed(search_entry.get_text());
            });
            
            search_entry.activate.connect(() => {
                activate_selected();
            });
            
            // Use capture phase to intercept arrow keys before SearchEntry processes them
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
            // Always handle Ctrl shortcuts for text editing
            if ((state & Gdk.ModifierType.CONTROL_MASK) != 0) {
                // Let Ctrl+A, Ctrl+C, Ctrl+V, etc. pass through to search entry
                return false;
            }
            
            // Escape key - exit grid mode if active, otherwise close
            if (keyval == Gdk.Key.Escape) {
                if (grid_mode) {
                    grid_mode = false;
                    exit_grid_mode();
                    return true;
                }
                close_requested();
                return true;
            }
            
            // If in grid mode, send ALL arrow keys to grid
            if (grid_mode) {
                if (keyval == Gdk.Key.Up) {
                    print("SearchBar: Arrow Up in grid mode\n");
                    arrow_up();
                    return true;
                }
                if (keyval == Gdk.Key.Down) {
                    print("SearchBar: Arrow Down in grid mode\n");
                    arrow_down();
                    return true;
                }
                if (keyval == Gdk.Key.Left) {
                    print("SearchBar: Arrow Left in grid mode\n");
                    arrow_left();
                    return true;
                }
                if (keyval == Gdk.Key.Right) {
                    print("SearchBar: Arrow Right in grid mode\n");
                    arrow_right();
                    return true;
                }
            }
            
            // Not in grid mode - only Down arrow enters grid mode
            if (keyval == Gdk.Key.Down) {
                grid_mode = true;
                arrow_down();
                return true;
            }
            
            // All other keys (including Up/Left/Right) work normally for text editing
            return false;
        }
        
        public void grab_focus() {
            search_entry.grab_focus();
        }
    }
}
