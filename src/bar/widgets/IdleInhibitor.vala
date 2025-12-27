using Gtk;
using Gdk;
using Pango;
using GLib;

namespace TopBar.Widgets {
    public class IdleInhibitor : Button {
        private State.AppState app_state = State.AppState.get_instance ();
        private Gtk.Image icon;

        public IdleInhibitor() {
            this.valign = Align.CENTER;

            icon = new Gtk.Image();
            this.set_css_classes({"square-icon"});
            this.set_child(icon);

            if (!app_state.is_idle_inhibitor_available()) {
                this.sensitive = false;
                this.icon.set_from_icon_name("dialog-error");
                this.tooltip_text = "Error: Idle inhibitor not available";
            }

            app_state.notify["idle-inhibitor-active"].connect(update_icon);

            this.clicked.connect(toggle_inhibitor);
            update_icon();
        }

        private void toggle_inhibitor() {
            app_state.idle_inhibitor_active = !app_state.idle_inhibitor_active;
        }

        private void update_icon() {
            if (app_state.idle_inhibitor_active) {
                icon.set_from_icon_name("caffeine-cup-full");
                this.tooltip_text = "Active";
            } else {
                icon.set_from_icon_name("caffeine-cup-empty");
                this.tooltip_text = "Inactive";
            }
        }
    }
}
