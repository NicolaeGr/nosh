using Gtk;

namespace TopBar.Widgets {
    public class Battery : Gtk.Button {
        Gtk.Image icon = new Gtk.Image ();

        public Battery () {
            set_css_classes ({ "Battery", "square-icon" });
            set_halign (Align.CENTER);
            set_valign (Align.CENTER);
            set_child (icon);

            var bat = AstalBattery.get_default ();

            bat.bind_property ("battery-icon-name", icon, "icon-name", BindingFlags.SYNC_CREATE);
            var dp=Math.floor (bat.percentage * 100);
            set_tooltip_text (@"Battery: $dp%");
            bat.notify["percentage"].connect (() => {
                var p = Math.floor (bat.percentage * 100);
                set_tooltip_text (@"Battery: $p%");
            });
        }
    }
}
