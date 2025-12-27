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

            set_css_classes({"Bar", "min-h-6", "px-1", "py-0", "mx-1", "text-sm", "font-bold"});
            
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