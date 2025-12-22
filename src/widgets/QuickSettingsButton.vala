namespace Widgets {
    public class QuickSettingsButton : Gtk.Button {
        public TopBar.QuickSettings? settings_window { get; set; }

        public QuickSettingsButton () {
            label = "⚙️";
            set_css_classes({"QuickSettingsButton", "square-icon"});

            clicked.connect (() => {
                if (settings_window != null) {
                    settings_window.visible = !settings_window.visible;
                }
            });
        }
    }
}
