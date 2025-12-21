namespace Widgets {
    public class Battery : Gtk.Box {
        Gtk.Image icon = new Gtk.Image ();
        Gtk.Label label = new Gtk.Label ("");

        public Battery () {
            append (icon);
            append (label);
            set_css_classes ({ "Battery" });

            var bat = AstalBattery.get_default ();
            bat.bind_property ("is-present", this, "visible", BindingFlags.SYNC_CREATE);
            bat.bind_property ("battery-icon-name", icon, "icon-name", BindingFlags.SYNC_CREATE);
            bat.bind_property ("percentage", label, "label", BindingFlags.SYNC_CREATE, (_, src, ref trgt) => {
                var p = Math.floor (src.get_double () * 100);
                trgt.set_string (@"$p%");
                return true;
            });
        }
    }
}