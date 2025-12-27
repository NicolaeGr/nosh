using Gtk;

namespace QuickSettings.Widgets {
    public class WiFiDropdown : Gtk.Box {
        private AstalNetwork.Wifi wifi;
        private Gtk.Box content_box;
        private Gtk.ScrolledWindow scroll;

        public WiFiDropdown (AstalNetwork.Wifi wifi_obj) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            set_css_classes ({"dropdown"});
            hexpand = true;
            vexpand = false;

            wifi = wifi_obj;

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"header"});
            header.halign = Gtk.Align.FILL;
            header.margin_start = 8;
            header.margin_end = 8;
            header.margin_top = 8;
            header.margin_bottom = 8;

           
            var header_label = new Gtk.Label ("WiFi");
            header_label.halign = Gtk.Align.START;

            var wifi_switch = new Gtk.Switch ();
            wifi_switch.set_css_classes ({"small-switch"});
            wifi_switch.active = wifi.enabled;
            wifi_switch.halign = Gtk.Align.START;
            wifi_switch.valign = Gtk.Align.CENTER;
            wifi_switch.hexpand = true;
            
            wifi.bind_property ("enabled", wifi_switch, "active", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);


            var refresh_button = new Gtk.Button.from_icon_name ("view-refresh-symbolic");
            refresh_button.halign = Gtk.Align.END;
            refresh_button.set_size_request (24, 24);
            refresh_button.clicked.connect (() => {
                request_scan ();
            });

            var collapse_button = new Gtk.Button.from_icon_name ("go-up-symbolic");
            collapse_button.halign = Gtk.Align.END;
            collapse_button.set_size_request (24, 24);

            header.append (header_label);
            header.append (wifi_switch);
            header.append (refresh_button);
            header.append (collapse_button);

            append (header);

            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.set_css_classes ({"divider"});
            append (separator);

            scroll = new Gtk.ScrolledWindow ();
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC);
            scroll.vexpand = false;
            scroll.set_min_content_height (240);

            content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.halign = Gtk.Align.FILL;
   

            scroll.set_child (content_box);
            append (scroll);

            update_networks ();

            wifi.notify["access-points"].connect (() => {
                update_networks ();
            });

            collapse_button.clicked.connect (() => {
                this.visible = false;
            });
        }

        private void update_networks () {
            while (content_box.get_first_child () != null) {
                content_box.remove (content_box.get_first_child ());
            }

            var access_points = wifi.access_points;
            if (access_points == null || access_points.length () == 0) {
                var label = new Gtk.Label ("No WiFi networks available");
                label.set_css_classes ({"empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
                return;
            }

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
            item.set_css_classes ({"network-item"});
            item.halign = Gtk.Align.FILL;
            item.margin_start = 0;
            item.margin_end = 0;
            item.margin_top = 0;
            item.margin_bottom = 0;
            item.hexpand = true;

            var icon = new Gtk.Image.from_icon_name (get_signal_icon (ap.strength));
            icon.set_icon_size (Gtk.IconSize.NORMAL);

            var info = new Gtk.Box (Gtk.Orientation.VERTICAL, 2);
            info.hexpand = true;
            info.halign = Gtk.Align.FILL;

            var name_label = new Gtk.Label (ap.ssid);
            name_label.halign = Gtk.Align.START;
            name_label.set_css_classes ({"network-name"});
            name_label.add_css_class ("dim-label");

            info.append (name_label);

            var status_label = new Gtk.Label ("");
            status_label.halign = Gtk.Align.START;
            status_label.set_css_classes ({"network-status"});
            
            if (wifi.active_access_point == ap) {
                status_label.set_label ("Connected");
                status_label.add_css_class ("connected");
            } else {
                status_label.set_label ("Signal: " + ap.strength.to_string () + "%");
            }
            
            info.append (status_label);

            var button = new Gtk.Button ();
            button.set_child (item);
            button.halign = Gtk.Align.FILL;
            button.add_css_class ("network-button");

            button.clicked.connect (() => {
                if (wifi.active_access_point == ap) {
                    wifi.deactivate_connection.begin (null);
                } else {
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

        private void request_scan () {
            try {
                wifi.scan ();
                // After scan, update the display with a delay to allow results to populate
                GLib.Timeout.add (500, () => {
                    update_networks ();
                    return false;
                });
            } catch (Error e) {
                warning ("WiFi scan failed: %s", e.message);
                update_networks ();
            }
        }
    }
}
