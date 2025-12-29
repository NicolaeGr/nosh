namespace Notifications {
    public class Window : Astal.Window {

        public Window () {
            Object (
                anchor: Astal.WindowAnchor.TOP | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.OVERLAY,
                visible: true,
                namespace: "nosh-notifications-outer"
            );

            set_css_classes ({"Notifications-outer"});

            var container = new Widgets.NotificationsContainer ();
            set_child (container);
        }
    }
}
