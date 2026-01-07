namespace AppLauncher {
    [GtkTemplate (ui = "/com/nicolaegr/nosh/app_launcher/Window.ui")]
    public class Window : Astal.Window {
        [GtkChild]
        private unowned Widgets.SearchBar search_bar;
        [GtkChild]
        private unowned Widgets.AppGrid app_grid;
        [GtkChild]
        private unowned Gtk.Box background;
        [GtkChild]
        private unowned Gtk.Box main_container;
        
        private AppProvider app_provider;
        private State.AppState app_state;
        
        public Window() {
            Object();
        }
        
        construct {
            app_provider = AppProvider.get_instance();
            app_state = State.AppState.get_instance();
            
            // Set size request on main container
            main_container.set_size_request(500, 312); // 500px width, 16:10 = 312px height
            
            // Connect search bar signals
            search_bar.query_changed.connect(on_search_changed);
            search_bar.activate_selected.connect(on_activate_selected);
            search_bar.arrow_up.connect(() => app_grid.move_up());
            search_bar.arrow_down.connect(() => app_grid.move_down());
            search_bar.arrow_left.connect(() => app_grid.move_left());
            search_bar.arrow_right.connect(() => app_grid.move_right());
            search_bar.exit_grid_mode.connect(on_exit_grid_mode);
            search_bar.close_requested.connect(close_launcher);
            
            // Connect app grid signals
            app_grid.app_activated.connect(on_app_activated);
            
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
            var plugin_manager = PluginManager.get_instance();
            var plugin = plugin_manager.get_active_plugin(query);
            search_bar.set_plugin(plugin);
            
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
