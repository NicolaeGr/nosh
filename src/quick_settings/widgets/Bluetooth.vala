using Gtk;

namespace QuickSettings.Widgets {
    public class Bluetooth : LargeButton {
        private AstalBluetooth.Bluetooth bluetooth;

        public Bluetooth () {
            base ();
            add_css_class ("QuickSettings-bluetooth");

            bluetooth = AstalBluetooth.get_default ();

            if (bluetooth == null) {
                set_status ("Bluetooth");
                set_icon_name ("bluetooth-disabled-symbolic");
                return;
            }

            update_status ();

            bluetooth.notify["is-powered"].connect (() => {
                update_status ();
            });
        }

        private void update_status () {
            if (bluetooth.is_powered) {
                set_status ("Bluetooth");
                set_icon_name ("bluetooth-active-symbolic");
                set_icon_enabled (true);
            } else {
                set_status ("Bluetooth Off");
                set_icon_name ("bluetooth-disabled-symbolic");
                set_icon_enabled (false);
            }
        }
    }
}
