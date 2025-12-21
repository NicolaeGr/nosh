using Gtk;
using Gdk;
using Pango;
using GLib;

namespace Widgets {
    public class IdleInhibitor : Button {
        private Utils.IdleInhibitorManager manager;
        private Gtk.Image icon;

        public IdleInhibitor() {
            this.valign = Align.CENTER;

            icon = new Gtk.Image();
            this.set_css_classes({ "square-icon" });
            this.set_child(icon);

            manager = Utils.IdleInhibitorManager.get_instance();

            if (!manager.is_available()) {
                this.sensitive = false;
                this.icon.set_from_icon_name("dialog-error");
                this.tooltip_text = "Error: Idle inhibitor not available";
            }

            manager.notify["active"].connect(update_icon);

            this.clicked.connect(toggle_inhibitor);
            update_icon();
        }

        private void toggle_inhibitor() {
            manager.active = !manager.active;
        }

        private void update_icon() {
            if (manager.active) {
                icon.set_from_icon_name("caffeine-cup-full");
                this.tooltip_text = "Active";
            } else {
                icon.set_from_icon_name("caffeine-cup-empty");
                this.tooltip_text = "Inactive";
            }
        }
    }
}
