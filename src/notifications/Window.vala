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

            add_css_class ("Notifications-outer");

           try {
                var notifd = AstalNotifd.Notifd.get_default ();
            } catch (Error e) {
                warning ("Failed to initialize notifyd: %s\n", e.message);
            }
        }
    }
}
