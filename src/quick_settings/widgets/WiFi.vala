using Gtk;

namespace QuickSettings.Widgets {
    public class WiFi : LargeButton {
        private AstalNetwork.Network network;
        private AstalNetwork.Wifi wifi;
        private bool is_connected = false;

        public WiFi () {
            // Properly initialize parent
            base ();
            add_css_class ("QuickSettings-wifi");

            network = AstalNetwork.get_default ();
            wifi = network.wifi;

            if (wifi == null) {
                set_status ("WiFi");
                set_icon_name ("network-wireless-offline-symbolic");
                return;
            }

            // Set initial state
            update_status ();

            // Watch for changes
            wifi.notify["icon-name"].connect (() => {
                set_icon_name (wifi.icon_name ?? "network-wireless-offline-symbolic");
            });

            wifi.notify["active-access-point"].connect (() => {
                update_status ();
            });

            wifi.notify["ssid"].connect (() => {
                update_status ();
            });
        }

        private void update_status () {
            is_connected = wifi.active_access_point != null;
            
            // Set icon
            var icon = wifi.icon_name ?? "network-wireless-offline-symbolic";
            set_icon_name (icon);
            
            // Set status text and color
            if (is_connected) {
                set_status (wifi.ssid ?? "WiFi");
                set_icon_enabled (true);
            } else {
                set_status ("WiFi Off");
                set_icon_enabled (false);
            }
        }
    }
}
