namespace Notifications {
    public class Window : Astal.Window {

        public Window () {
            Object (
                anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.OVERLAY,
                visible: false,
                namespace: "nosh-notifications-outer",
                margin_top: 8,
                margin_right: 28
            );

            set_css_classes ({"Notifications-outer"});

            var container = new Widgets.NotificationsContainer ();
            set_child (container);
        }
    }
}
