using Gtk;

namespace Widgets {
    public class Battery : Gtk.Button {
        Gtk.Image icon = new Gtk.Image ();

        public Battery () {
            set_css_classes ({ "Battery", "square-icon" });
            set_halign (Align.CENTER);
            set_valign (Align.CENTER);
            set_child (icon);

            //  this widget does not show at all now so let's add something for testing
            var label = new Gtk.Label ("Battery");
            set_child (label);

            var bat = AstalBattery.get_default ();
            bat.bind_property ("is-present", this, "visible", BindingFlags.SYNC_CREATE);
            bat.bind_property ("battery-icon-name", icon, "icon-name", BindingFlags.SYNC_CREATE);
            bat.notify["percentage"].connect (() => {
                var p = Math.floor (bat.percentage * 100);
                set_tooltip_text (@"Battery: $p%");
            });
        }
    }
}