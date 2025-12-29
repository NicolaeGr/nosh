namespace AppLauncher {
    public class Window : Astal.Window {
        private Widgets.SearchBar search_bar;
        private Widgets.AppGrid app_grid;
        private AppProvider app_provider;
        private State.AppState app_state;
        
        public Window() {
            Object(
                anchor: Astal.WindowAnchor.TOP
                    | Astal.WindowAnchor.BOTTOM
                    | Astal.WindowAnchor.LEFT
                    | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.OVERLAY,
                keymode: Astal.Keymode.EXCLUSIVE,
                visible: false,
                namespace: "nosh-app-launcher"
            );
            
            set_css_classes({"AppLauncher"});
            
            app_provider = AppProvider.get_instance();
            app_state = State.AppState.get_instance();
            
            // Background overlay
            var background = new Gtk.Box(Gtk.Orientation.VERTICAL, 0);
            background.hexpand = true;
            background.vexpand = true;
            background.set_css_classes({"background"});
            
            // Main container with max width and aspect ratio
            var main_container = new Gtk.Box(Gtk.Orientation.VERTICAL, 0);
            main_container.set_css_classes({"container"});
            main_container.set_size_request(500, 312); // 500px width, 16:10 = 312px height
            main_container.valign = Gtk.Align.CENTER;
            main_container.halign = Gtk.Align.CENTER;
            
            // Search bar
            search_bar = new Widgets.SearchBar();
            search_bar.search_changed.connect(on_search_changed);
            search_bar.activate_selected.connect(on_activate_selected);
            search_bar.move_next.connect(() => app_grid.select_next());
            search_bar.move_previous.connect(() => app_grid.select_previous());
            main_container.append(search_bar);
            
            // App grid
            app_grid = new Widgets.AppGrid();
            app_grid.app_activated.connect(on_app_activated);
            main_container.append(app_grid);
            
            background.append(main_container);
            set_child(background);
            
            // Close on background click
            var bg_click = new Gtk.GestureClick();
            bg_click.pressed.connect((n_press, x, y) => {
                if (background.pick(x, y, Gtk.PickFlags.DEFAULT) == background) {
                    close_launcher();
                }
            });
            background.add_controller(bg_click);
            
            // Escape key to close
            var key_controller = new Gtk.EventControllerKey();
            key_controller.key_pressed.connect((keyval, keycode, state) => {
                if (keyval == Gdk.Key.Escape) {
                    close_launcher();
                    return true;
                }
                return false;
            });
            ((Gtk.Widget) this).add_controller(key_controller);
            
            // Bind to app state
            app_state.bind_property("app-launcher-open", this, "visible", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);
            
            // Initialize with all apps
            notify["visible"].connect(() => {
                if (visible) {
                    search_bar.set_text("");
                    app_grid.set_apps(app_provider.get_all_apps());
                    present();
                    search_bar.grab_focus();
                }
            });
        }
        
        private void on_search_changed(string query) {
            var results = app_provider.search(query);
            app_grid.set_apps(results);
        }
        
        private void on_activate_selected() {
            app_grid.activate_selected();
        }
        
        private void on_app_activated(AppEntry entry) {
            app_provider.record_launch(entry);
            entry.launch();
            close_launcher();
        }
        
        private void close_launcher() {
            app_state.app_launcher_open = false;
        }
        
        public void toggle() {
            visible = !visible;
        }
    }
}
