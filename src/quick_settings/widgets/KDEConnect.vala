using Gtk;

namespace QuickSettings.Widgets {
    public class KDEConnect : LargeButton {
        public KDEConnect () {
            base ();
            add_css_class ("QuickSettings-kde-connect");

            set_icon_name ("smartphone-symbolic");
            set_status ("KDE Connect");
            set_icon_enabled (true);
        }
    }
}
