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

            // Header with switch and collapse button
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"QuickSettings-dropdown-header"});
            header.halign = Gtk.Align.FILL;
            header.margin_start = 8;
            header.margin_end = 8;
            header.margin_top = 8;
            header.margin_bottom = 8;

            var header_label = new Gtk.Label ("Bluetooth");
            header_label.hexpand = true;
            header_label.halign = Gtk.Align.START;

            var collapse_button = new Gtk.Button.from_icon_name ("go-up-symbolic");
            collapse_button.halign = Gtk.Align.END;

            header.append (header_label);
            header.append (collapse_button);

            append (header);

            // Settings button
            var settings_button = new Gtk.Button.with_label ("Bluetooth Settings");
            settings_button.halign = Gtk.Align.FILL;
            settings_button.margin_start = 8;
            settings_button.margin_end = 8;
            settings_button.margin_bottom = 4;
            append (settings_button);

            // Separator
            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.set_css_classes ({"QuickSettings-divider"});
            append (separator);

            // Scrollable content area for devices
            scroll = new Gtk.ScrolledWindow ();
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC);
            scroll.vexpand = false;
            scroll.set_min_content_height (240);

            content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.halign = Gtk.Align.FILL;

            scroll.set_child (content_box);
            append (scroll);

            // Populate with paired devices
            update_devices ();

            // Connect signals
            bluetooth.notify["is-powered"].connect (() => {
                update_devices ();
            });

            bluetooth.notify["devices"].connect (() => {
                update_devices ();
            });

            collapse_button.clicked.connect (() => {
                // Hide the dropdown by setting visible to false
                this.visible = false;
            });
        }

        private void update_devices () {
            // Clear existing items
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

            // Get paired devices and display them
            bool has_devices = false;
            foreach (var device in bluetooth.devices) {
                if (device.paired) {
                    var item = create_device_item (device);
                    content_box.append (item);
                    has_devices = true;
                }
            }

            if (!has_devices) {
                var label = new Gtk.Label ("No paired devices");
                label.set_css_classes ({"QuickSettings-empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
            }
        }

        private Gtk.Box create_device_item (AstalBluetooth.Device device) {
            var item = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            item.set_css_classes ({"QuickSettings-device-item"});
            item.halign = Gtk.Align.FILL;
            item.margin_start = 8;
            item.margin_end = 8;
            item.margin_top = 8;
            item.margin_bottom = 8;

            // Device icon
            var icon = new Gtk.Image.from_icon_name (get_device_icon (device.icon));
            icon.set_icon_size (Gtk.IconSize.LARGE);

            // Device name and status
            var info = new Gtk.Box (Gtk.Orientation.VERTICAL, 2);
            info.hexpand = true;
            info.halign = Gtk.Align.FILL;

            var name_label = new Gtk.Label (device.name);
            name_label.halign = Gtk.Align.START;
            name_label.set_css_classes ({"QuickSettings-device-name"});

            var status_label = new Gtk.Label (device.connected ? "Connected" : "Disconnected");
            status_label.halign = Gtk.Align.START;
            status_label.set_css_classes ({"QuickSettings-device-status"});
            status_label.add_css_class ("dim-label");

            info.append (name_label);
            info.append (status_label);

            // Connection toggle
            var connect_switch = new Gtk.Switch ();
            connect_switch.active = device.connected;
            connect_switch.halign = Gtk.Align.END;
            connect_switch.valign = Gtk.Align.CENTER;
            connect_switch.notify["active"].connect (() => {
                if (connect_switch.active) {
                    device.connect_device.begin ();
                } else {
                    device.disconnect_device.begin ();
                }
            });

            item.append (icon);
            item.append (info);
            item.append (connect_switch);

            return item;
        }

        private string get_device_icon (string icon_name) {
            if (icon_name != null && icon_name.length > 0) {
                return icon_name;
            }
            return "bluetooth-symbolic";
        }
    }
}
