using Gtk;

namespace QuickSettings.Widgets {
    public class Connectivity : Gtk.Box {
        private WiFiDropdown wifi_dropdown;
        private BluetoothDropdown bt_dropdown;
        private KDEConnectDropdown kde_dropdown;
        private Gtk.Box dropdowns_container;
        private Gtk.CssProvider height_provider = new Gtk.CssProvider ();
        private State.AppState app_state = State.AppState.get_instance ();

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

            dropdowns_container = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            dropdowns_container.set_css_classes ({"QuickSettings-dropdowns-container"});
            dropdowns_container.halign = Gtk.Align.FILL;
            append (dropdowns_container);

            if (network != null && network.wifi != null) {
                wifi_dropdown = new WiFiDropdown (network.wifi);
                wifi_dropdown.visible = false;
                dropdowns_container.append (wifi_dropdown);

                wifi.button.clicked.connect (() => {
                    toggle_dropdown (wifi_dropdown);
                    hide_other_dropdowns (wifi_dropdown);
                });
            }

            if (bluetooth != null) {
                bt_dropdown = new BluetoothDropdown (bluetooth);
                bt_dropdown.visible = false;
                dropdowns_container.append (bt_dropdown);

                bt.button.clicked.connect (() => {
                    toggle_dropdown (bt_dropdown);
                    hide_other_dropdowns (bt_dropdown);
                });
            }

            kde_dropdown = new KDEConnectDropdown ();
            kde_dropdown.visible = false;
            dropdowns_container.append (kde_dropdown);

            kde.button.clicked.connect (() => {
                toggle_dropdown (kde_dropdown);
                hide_other_dropdowns (kde_dropdown);
            });

            // Close all dropdowns when quick settings window closes
            app_state.notify["quick-settings-open"].connect (() => {
                if (!app_state.quick_settings_open) {
                    hide_other_dropdowns (null);
                }
            });
        }

        private void toggle_dropdown (Gtk.Widget dropdown) {
            if (dropdown.visible) {
                Gtk.Requisition min_size, natural_size;
                dropdown.get_preferred_size (out min_size, out natural_size);
                int current_height = dropdown.get_allocated_height ();
                if (current_height <= 0) {
                    current_height = natural_size.height;
                }
                
                try {
                    var css = "* { max-height: %dpx !important; }".printf (current_height);
                    height_provider.load_from_data (css.data);
                    dropdown.get_style_context ().add_provider (height_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
                } catch (Error e) {
                    warning ("Failed to set height: %s", e.message);
                }
                
                dropdown.remove_css_class ("visible");
                
                GLib.Timeout.add (10, () => {
                    try {
                        height_provider.load_from_data ("* { max-height: 0 !important; }".data);
                        dropdown.get_style_context ().add_provider (height_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
                    } catch (Error e) {
                        warning ("Failed to animate close: %s", e.message);
                    }
                    return false;
                });
                
                GLib.Timeout.add (310, () => {
                    dropdown.visible = false;
                    return false;
                });
            } else {
                dropdown.visible = true;
                
                GLib.Idle.add (() => {
                    Gtk.Requisition min_size, natural_size;
                    dropdown.get_preferred_size (out min_size, out natural_size);
                    
                    int allocated_height = dropdown.get_allocated_height ();
                    int height = natural_size.height > 0 ? natural_size.height : allocated_height;
                    
                    if (height > 0) {
                        try {
                            var css = "* { max-height: %dpx !important; }".printf (height + 50); 
                            height_provider.load_from_data (css.data);
                            dropdown.get_style_context ().add_provider (height_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
                        } catch (Error e) {
                            warning ("Failed to set dropdown height: %s", e.message);
                        }
                    }
                    
                    dropdown.add_css_class ("visible");
                    return false;
                });
            }
        }

        private void hide_other_dropdowns (Gtk.Widget? except) {
            if (wifi_dropdown != null && wifi_dropdown != except) {
                wifi_dropdown.visible = false;
                wifi_dropdown.remove_css_class ("visible");
            }
            if (bt_dropdown != null && bt_dropdown != except) {
                bt_dropdown.visible = false;
                bt_dropdown.remove_css_class ("visible");
            }
            if (kde_dropdown != null && kde_dropdown != except) {
                kde_dropdown.visible = false;
                kde_dropdown.remove_css_class ("visible");
            }
        }
    }
}


