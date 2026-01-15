using Gtk;

namespace TopBar.Widgets {
    public class Wifi : Gtk.Button {
        private Gtk.Image icon;

        public Wifi () {
            set_css_classes ({"square-icon" });
            set_valign (Align.CENTER);

            icon = new Gtk.Image ();
            this.set_child(icon);

            var network = AstalNetwork.get_default ();
            var wifi = network != null ? network.wifi : null;
            
            if (wifi != null) {
                wifi.bind_property ("ssid", this, "tooltip-text", BindingFlags.SYNC_CREATE);
                wifi.bind_property ("icon-name", icon, "icon-name", BindingFlags.SYNC_CREATE);
                this.visible = true;
            } else {
                this.visible = false;
            }
        }
    }
}
