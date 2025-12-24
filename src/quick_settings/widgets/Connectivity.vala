using Gtk;

namespace QuickSettings.Widgets {
    public class Connectivity : Gtk.Box {
        private WiFiDropdown wifi_dropdown;
        private BluetoothDropdown bt_dropdown;
        private KDEConnectDropdown kde_dropdown;

        public Connectivity () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);

            var connectivity_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            connectivity_box.set_css_classes ({"QuickSettings-item-row"});
            connectivity_box.halign = Gtk.Align.FILL;
            connectivity_box.homogeneous = true;

            // Create connectivity widgets
            var network = AstalNetwork.get_default ();
            var wifi = new WiFi ();
            
            var bluetooth = AstalBluetooth.get_default ();
            var bt = new Bluetooth ();
            
            var kde = new KDEConnect ();

            connectivity_box.append (wifi);
            connectivity_box.append (bt);
            connectivity_box.append (kde);

            append (connectivity_box);

            // Create and add dropdowns
            if (network != null && network.wifi != null) {
                wifi_dropdown = new WiFiDropdown (network.wifi);
                wifi_dropdown.visible = false;
                append (wifi_dropdown);

                // Connect wifi icon click to toggle dropdown
                wifi.button.clicked.connect (() => {
                    wifi_dropdown.visible = !wifi_dropdown.visible;
                    hide_other_dropdowns (wifi_dropdown);
                });
            }

            if (bluetooth != null) {
                bt_dropdown = new BluetoothDropdown (bluetooth);
                bt_dropdown.visible = false;
                append (bt_dropdown);

                // Connect bluetooth icon click to toggle dropdown
                bt.button.clicked.connect (() => {
                    bt_dropdown.visible = !bt_dropdown.visible;
                    hide_other_dropdowns (bt_dropdown);
                });
            }

            kde_dropdown = new KDEConnectDropdown ();
            kde_dropdown.visible = false;
            append (kde_dropdown);

            // Connect KDE Connect icon click to toggle dropdown
            kde.button.clicked.connect (() => {
                kde_dropdown.visible = !kde_dropdown.visible;
                hide_other_dropdowns (kde_dropdown);
            });
        }

        private void hide_other_dropdowns (Gtk.Widget except) {
            if (wifi_dropdown != null && wifi_dropdown != except) {
                wifi_dropdown.visible = false;
            }
            if (bt_dropdown != null && bt_dropdown != except) {
                bt_dropdown.visible = false;
            }
            if (kde_dropdown != null && kde_dropdown != except) {
                kde_dropdown.visible = false;
            }
        }
    }
}
