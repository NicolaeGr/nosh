using Gtk;

namespace QuickSettings.Widgets {
    public class BluetoothDropdown : Gtk.Box {
        private AstalBluetooth.Bluetooth bluetooth;
        private Gtk.Box content_box;
        private Gtk.ScrolledWindow scroll;

        public BluetoothDropdown (AstalBluetooth.Bluetooth bt_obj) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            set_css_classes ({"QuickSettings-dropdown"});
            hexpand = true;
            vexpand = false;

            bluetooth = bt_obj;

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"QuickSettings-dropdown-header"});
            header.halign = Gtk.Align.FILL;
            header.margin_start = 8;
            header.margin_end = 8;
            header.margin_top = 8;
            header.margin_bottom = 8;

            var header_label = new Gtk.Label ("Bluetooth");
            header_label.halign = Gtk.Align.START;

            var bt_switch = new Gtk.Switch ();
            bt_switch.set_css_classes ({"small-switch"});
            bt_switch.active = bluetooth.is_powered;
            bt_switch.halign = Gtk.Align.START;
            bt_switch.valign = Gtk.Align.CENTER;
            bt_switch.hexpand = true;
            
            bluetooth.bind_property ("is-powered", bt_switch, "active", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);

            var refresh_button = new Gtk.Button.from_icon_name ("view-refresh-symbolic");
            refresh_button.halign = Gtk.Align.END;
            refresh_button.set_size_request (24, 24);
            refresh_button.clicked.connect (() => {
                update_devices ();
            });

            var settings_button = new Gtk.Button.from_icon_name ("preferences-system-symbolic");
            settings_button.halign = Gtk.Align.END;
            settings_button.set_size_request (24, 24);
            settings_button.clicked.connect (() => {
                try {
                    GLib.AppInfo.launch_default_for_uri ("settings://bluetooth", null);
                } catch (Error e) {
                    warning ("Failed to open Bluetooth settings: %s", e.message);
                    try {
                        GLib.Process.spawn_command_line_async ("gnome-control-center bluetooth");
                    } catch (Error e2) {
                        warning ("Failed to launch bluetooth settings: %s", e2.message);
                    }
                }
            });

            var collapse_button = new Gtk.Button.from_icon_name ("go-up-symbolic");
            collapse_button.halign = Gtk.Align.END;
            collapse_button.set_size_request (24, 24);

            header.append (header_label);
            header.append (bt_switch);
            header.append (refresh_button);
            header.append (settings_button);
            header.append (collapse_button);

            append (header);

            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.set_css_classes ({"QuickSettings-divider"});
            append (separator);

            scroll = new Gtk.ScrolledWindow ();
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC);
            scroll.vexpand = false;
            scroll.set_min_content_height (240);

            content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.halign = Gtk.Align.FILL;

            scroll.set_child (content_box);
            append (scroll);

            update_devices ();

            bluetooth.notify["is-powered"].connect (() => {
                update_devices ();
            });

            bluetooth.notify["devices"].connect (() => {
                update_devices ();
            });

            collapse_button.clicked.connect (() => {
                this.visible = false;
            });
        }

        private void update_devices () {
            while (content_box.get_first_child () != null) {
                content_box.remove (content_box.get_first_child ());
            }

            if (!bluetooth.is_powered) {
                var label = new Gtk.Label ("Bluetooth is off");
                label.set_css_classes ({"QuickSettings-empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
                return;
            }

            var paired_devices = new GLib.List<AstalBluetooth.Device> ();
            foreach (var device in bluetooth.devices) {
                if (device.paired) {
                    paired_devices.append (device);
                }
            }
            
            paired_devices.sort ((a, b) => {
                if (a.connected && !b.connected) return -1;
                if (!a.connected && b.connected) return 1;
                return 0;
            });
            
            bool has_devices = false;
            foreach (var device in paired_devices) {
                var item = create_device_item (device);
                content_box.append (item);
                has_devices = true;
            }

            if (!has_devices) {
                var label = new Gtk.Label ("No paired devices");
                label.set_css_classes ({"QuickSettings-empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
            }
        }

        private Gtk.Button create_device_item (AstalBluetooth.Device device) {
            var item = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 4);
            item.set_css_classes ({"QuickSettings-device-item"});
            item.halign = Gtk.Align.FILL;
            item.margin_start = 0;
            item.margin_end = 0;
            item.margin_top = 0;
            item.margin_bottom = 0;
            item.hexpand = true;

            var icon = new Gtk.Image.from_icon_name (get_device_icon (device.icon));
            icon.set_icon_size (Gtk.IconSize.NORMAL);

            var info = new Gtk.Box (Gtk.Orientation.VERTICAL, 2);
            info.hexpand = true;
            info.halign = Gtk.Align.FILL;

            var name_label = new Gtk.Label (device.name);
            name_label.halign = Gtk.Align.START;
            name_label.set_css_classes ({"QuickSettings-network-name"});
            name_label.add_css_class ("dim-label");

            info.append (name_label);

            var status_label = new Gtk.Label ("");
            status_label.halign = Gtk.Align.START;
            status_label.set_css_classes ({"QuickSettings-network-status"});
            
            if (device.connected) {
                status_label.set_label ("Connected");
                status_label.add_css_class ("connected");
            } else {
                status_label.set_label ("Available");
            }
            
            info.append (status_label);

            var button = new Gtk.Button ();
            button.set_child (item);
            button.halign = Gtk.Align.FILL;
            button.add_css_class ("QuickSettings-network-button");

            button.clicked.connect (() => {
                if (device.connected) {
                    device.disconnect_device.begin ();
                } else {
                    device.connect_device.begin ();
                }
            });

            item.append (icon);
            item.append (info);

            return button;
        }

        private string get_device_icon (string icon_name) {
            if (icon_name != null && icon_name.length > 0) {
                return icon_name;
            }
            return "bluetooth-symbolic";
        }
    }
}
