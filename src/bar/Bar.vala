namespace TopBar {
    public class Bar : Astal.Window {
        public Bar () {
            Object (
                    anchor: Astal.WindowAnchor.TOP
                    | Astal.WindowAnchor.LEFT
                    | Astal.WindowAnchor.RIGHT,
                    exclusivity: Astal.Exclusivity.EXCLUSIVE,
                    namespace: "hypr-shell-topbar"
            );

            add_css_class ("Bar");

            var layout = new Gtk.CenterBox ();
            layout.set_start_widget (new Left ());
            layout.set_center_widget (new Center ());
            layout.set_end_widget (new Right ());

            set_child (layout);
            present ();
        }
    }
}