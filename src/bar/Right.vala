namespace TopBar {
    public class Right : Gtk.Box {
        public Right () {
            hexpand = true;
            halign = Gtk.Align.END;

            append (new Widgets.SysTray ());
            append (new Widgets.IdleInhibitor ());
            append (new Widgets.Wifi ());
            append (new Widgets.AudioSlider ());
            append (new Widgets.Battery ());
        }
    }
}