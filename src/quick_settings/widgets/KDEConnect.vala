using Gtk;
using KdeConnect;

namespace QuickSettings.Widgets {
    public class KDEConnect : LargeButton {
        private KdeConnect.Manager kde_manager;

        public KDEConnect () {
            base ();
            add_css_class ("QuickSettings-kde-connect");

            set_icon_name ("smartphone-symbolic");
            set_status ("KDE Connect");
            set_icon_enabled (true);
            
            // Create manager to monitor daemon state
            kde_manager = new KdeConnect.Manager();
            
            kde_manager.daemon_state_changed.connect((running) => {
                update_status(running);
            });
        }
        
        private void update_status(bool running) {
            if (running) {
                set_status("KDE Connect");
                set_icon_enabled(true);
                status_label.remove_css_class("disabled");
            } else {
                set_status("KDE Connect (Off)");
                set_icon_enabled(false);
                status_label.add_css_class("disabled");
            }
        }
    }
}

