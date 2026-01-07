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
            search_bar.query_changed.connect(on_search_changed);
            search_bar.activate_selected.connect(on_activate_selected);
            search_bar.arrow_up.connect(() => app_grid.move_up());
            search_bar.arrow_down.connect(() => app_grid.move_down());
            search_bar.arrow_left.connect(() => app_grid.move_left());
            search_bar.arrow_right.connect(() => app_grid.move_right());
            search_bar.exit_grid_mode.connect(on_exit_grid_mode);
            search_bar.close_requested.connect(close_launcher);
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
            
            // Bind to app state
            app_state.bind_property("app_launcher_open", this, "visible", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);
            
            // Initialize with all apps
            notify["visible"].connect(() => {
                if (visible) {
                    app_provider.reload();
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
        
        private void on_exit_grid_mode() {
            // Reset selection to first item when exiting grid mode
            app_grid.selected_index = 0;
            app_grid.update_selection();
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
