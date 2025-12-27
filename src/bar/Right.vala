namespace TopBar {
    public class Right : Gtk.Box {
        public Right () {
            hexpand = true;
            halign = Gtk.Align.END;
            set_css_classes ({ "Right" });

            append (new Widgets.SystemStats ());
            append (new Widgets.SysTray ());
            append (new Widgets.IdleInhibitor ());
            append (new Widgets.Battery ());
            append (new Widgets.Wifi ());
            append (new Widgets.Volume ());
            append (new Widgets.QuickSettingsButton ());
        }
    }
}