namespace TopBar {
    public class Center : Gtk.Box {
        public Center () {
            append (new Widgets.Time ());
            append (new Widgets.Media ());
        }
    }
}