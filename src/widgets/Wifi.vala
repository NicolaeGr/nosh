namespace Widgets {
    public class Wifi : Gtk.Box {
        private Gtk.Image icon;

        public Wifi () {
            set_css_classes ({ "Wifi" });

            icon = new Gtk.Image ();
            icon.hexpand = false;
            icon.halign = Gtk.Align.CENTER;
            append (icon);

            var wifi = AstalNetwork.get_default ().wifi;
            if (wifi != null) {
                wifi.bind_property ("ssid", this, "tooltip-text", BindingFlags.SYNC_CREATE);
                wifi.bind_property ("icon-name", icon, "icon-name", BindingFlags.SYNC_CREATE);
            }
        }
    }
}