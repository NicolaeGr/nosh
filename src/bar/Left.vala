namespace TopBar {
    public class Left : Gtk.Box {
        public Left () {
            hexpand = true;
            halign = Gtk.Align.START;
            append (new Widgets.Workspaces ());
        }
    }
}