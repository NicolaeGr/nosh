namespace TopBar.Widgets {
    public class QuickSettingsButton : Gtk.Button {
        private State.AppState app_state = State.AppState.get_instance ();

        public QuickSettingsButton () {
            label = "⚙️";
            set_css_classes({"QuickSettingsButton", "square-icon"});

            clicked.connect (() => {
                app_state.quick_settings_open = !app_state.quick_settings_open;
            });
        }
    }
}
