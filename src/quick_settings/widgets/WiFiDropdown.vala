using Gtk;

namespace QuickSettings.Widgets {
    public class WiFiDropdown : Gtk.Box {
        private AstalNetwork.Wifi wifi;
        private Gtk.Box content_box;
        private Gtk.ScrolledWindow scroll;

        public WiFiDropdown (AstalNetwork.Wifi wifi_obj) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            set_css_classes ({"QuickSettings-dropdown"});
            hexpand = true;
            vexpand = false;

            wifi = wifi_obj;

            // Header with switch and collapse button
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"QuickSettings-dropdown-header"});
            header.halign = Gtk.Align.FILL;
            header.margin_start = 8;
            header.margin_end = 8;
            header.margin_top = 8;
            header.margin_bottom = 8;

            var header_label = new Gtk.Label ("WiFi");
            header_label.hexpand = true;
            header_label.halign = Gtk.Align.START;

            var collapse_button = new Gtk.Button.from_icon_name ("go-up-symbolic");
            collapse_button.halign = Gtk.Align.END;

            header.append (header_label);
            header.append (collapse_button);

            append (header);

            // Settings button
            var settings_button = new Gtk.Button.with_label ("WiFi Settings");
            settings_button.halign = Gtk.Align.FILL;
            settings_button.margin_start = 8;
            settings_button.margin_end = 8;
            settings_button.margin_bottom = 4;
            append (settings_button);

            // Separator
            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.set_css_classes ({"QuickSettings-divider"});
            append (separator);

            // Scrollable content area for WiFi networks
            scroll = new Gtk.ScrolledWindow ();
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC);
            scroll.vexpand = false;
            scroll.set_min_content_height (240);

            content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.halign = Gtk.Align.FILL;
   

            scroll.set_child (content_box);
            append (scroll);

            // Populate with available networks
            update_networks ();

            // Connect signal to update when access points change
            wifi.notify["access-points"].connect (() => {
                update_networks ();
            });

            collapse_button.clicked.connect (() => {
                // Hide the dropdown by setting visible to false
                this.visible = false;
            });
        }

        private void update_networks () {
            // Clear existing items
            while (content_box.get_first_child () != null) {
                content_box.remove (content_box.get_first_child ());
            }

            // Get access points and display them
            var access_points = wifi.access_points;
            if (access_points == null || access_points.length () == 0) {
                var label = new Gtk.Label ("No WiFi networks available");
                label.set_css_classes ({"QuickSettings-empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
                return;
            }

            // Sort by signal strength (descending)
            var sorted_aps = new GLib.List<AstalNetwork.AccessPoint> ();
            foreach (var ap in access_points) {
                sorted_aps.append (ap);
            }
            sorted_aps.sort ((a, b) => {
                return (int)(b.strength - a.strength);
            });

            foreach (var ap in sorted_aps) {
                var item = create_network_item (ap);
                content_box.append (item);
            }
        }

        private Gtk.Button create_network_item (AstalNetwork.AccessPoint ap) {
            var item = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 4);
            item.set_css_classes ({"QuickSettings-network-item"});
            item.halign = Gtk.Align.FILL;
            item.margin_start = 8;
            item.margin_end = 8;
            item.margin_top = 4;
            item.margin_bottom = 4;

            // Network icon based on strength - SMALL size
            var icon = new Gtk.Image.from_icon_name (get_signal_icon (ap.strength));
            icon.set_icon_size (Gtk.IconSize.NORMAL);

            // Network name and status
            var info = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            info.hexpand = true;
            info.halign = Gtk.Align.FILL;

            var name_label = new Gtk.Label (ap.ssid);
            name_label.halign = Gtk.Align.START;
            name_label.set_css_classes ({"QuickSettings-network-name"});
            name_label.add_css_class ("dim-label");

            info.append (name_label);

            // Show "Connected" status if this is the active network
            if (wifi.active_access_point == ap) {
                var status_label = new Gtk.Label ("Connected");
                status_label.halign = Gtk.Align.START;
                status_label.set_css_classes ({"success"});
                info.append (status_label);
            }

            // Make the entire item clickable to connect
            var button = new Gtk.Button ();
            button.set_child (item);
            button.halign = Gtk.Align.FILL;

            button.clicked.connect (() => {
                if (wifi.active_access_point != ap) {
                    ap.activate.begin (null);
                }
            });

            item.append (icon);
            item.append (info);

            return button;
        }

        private string get_signal_icon (uint8 strength) {
            if (strength >= 80) {
                return "network-wireless-signal-excellent-symbolic";
            } else if (strength >= 60) {
                return "network-wireless-signal-good-symbolic";
            } else if (strength >= 40) {
                return "network-wireless-signal-ok-symbolic";
            } else if (strength >= 20) {
                return "network-wireless-signal-weak-symbolic";
            } else {
                return "network-wireless-signal-none-symbolic";
            }
        }
    }
}
