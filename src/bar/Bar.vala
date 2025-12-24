namespace TopBar {
    public class Bar : Astal.Window {
        public Bar (bool is_secondary = false) {
            Object (
                    anchor: Astal.WindowAnchor.TOP
                    | Astal.WindowAnchor.LEFT
                    | Astal.WindowAnchor.RIGHT,
                    exclusivity: Astal.Exclusivity.EXCLUSIVE,
                    namespace: "nosh-topbar"
            );

            add_css_class ("Bar");
            
            // Add secondary indicator class if this is the dev instance
            if (is_secondary) {
                add_css_class ("secondary-instance");
            }

            var layout = new Gtk.CenterBox ();
            layout.set_start_widget (new Left ());
            layout.set_center_widget (new Center ());
            layout.set_end_widget (new Right ());

            set_child (layout);
            present ();
        }
    }
}