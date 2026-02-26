namespace AppLauncher.Widgets {
    [GtkTemplate(ui = "/com/nicolaegr/nosh/app_launcher/widgets/AppGrid.ui")]
    public class AppGrid : Gtk.Box {
        [GtkChild]
        private unowned Gtk.ScrolledWindow scrolled_window;
        [GtkChild]
        private unowned Gtk.FlowBox flow_box;

        private Gee.ArrayList<AppEntry> apps;
        public int selected_index = 0;
        private const int COLUMNS = 2;

        public signal void app_activated(AppEntry entry);

        public AppGrid() {
            Object();
        }

        construct {

            flow_box.child_activated.connect((child) => {
                var index = child.get_index();
                if (index >= 0 && index < apps.size) {
                    activate_app(apps[index]);
                }
            });
        }

        public void set_apps(Gee.ArrayList<AppEntry> new_apps) {
            apps = new_apps;
            selected_index = 0;

            reset_scroll_position();

            var child = flow_box.get_first_child();
            while (child != null) {
                var next = child.get_next_sibling();
                flow_box.remove(child);
                child = next;
            }

            foreach (var app in apps) {
                var item = create_app_item(app);
                flow_box.append(item);
            }

            if (apps.size > 0) {
                var first_child = flow_box.get_child_at_index(0);
                if (first_child != null) {
                    flow_box.select_child(first_child);
                }
            }
        }

        private Gtk.Box create_app_item(AppEntry app) {
            var box = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 8);
            box.set_css_classes({ "AppItem" });

            var icon_widget = new Gtk.Image();
            if (app.icon != null) {
                icon_widget.set_from_icon_name(app.icon);
                icon_widget.set_pixel_size(32);
            } else {
                icon_widget.set_from_icon_name("application-x-executable");
                icon_widget.set_pixel_size(32);
            }
            box.append(icon_widget);

            var label = new Gtk.Label(app.name);
            label.set_ellipsize(Pango.EllipsizeMode.END);
            label.set_xalign(0);
            label.set_hexpand(true);
            box.append(label);

            return box;
        }

        public void move_up() {
            if (apps.size == 0)return;

            int new_index = selected_index - COLUMNS;
            if (new_index >= 0) {
                selected_index = new_index;
                update_selection();
            }
        }

        public void move_down() {
            if (apps.size == 0)return;

            int new_index = selected_index + COLUMNS;
            if (new_index < apps.size) {
                selected_index = new_index;
                update_selection();
            }
        }

        public void move_left() {
            if (apps.size == 0)return;

            if (selected_index % COLUMNS != 0) {
                selected_index--;
                update_selection();
            }
        }

        public void move_right() {
            if (apps.size == 0)return;

            if (selected_index % COLUMNS != COLUMNS - 1 && selected_index < apps.size - 1) {
                selected_index++;
                update_selection();
            }
        }

        public void update_selection() {
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                flow_box.select_child(child);
                scroll_to_selected();
            }
        }

        public void activate_selected() {
            if (selected_index >= 0 && selected_index < apps.size) {
                activate_app(apps[selected_index]);
            }
        }

        private void activate_app(AppEntry entry) {
            app_activated(entry);
        }

        private void scroll_to_selected() {
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                var adj = scrolled_window.get_vadjustment();

                Graphene.Point point;
                if (child.compute_point(flow_box, Graphene.Point.zero(), out point)) {
                    double child_y = point.y;

                    if (child_y < adj.get_value()) {
                        adj.set_value(child_y);
                    } else if (child_y + child.get_height() > adj.get_value() + adj.get_page_size()) {
                        adj.set_value(child_y + child.get_height() - adj.get_page_size());
                    }
                }
            }
        }

        private void reset_scroll_position() {
            var adj = scrolled_window.get_vadjustment();
            adj.set_value(0.0);
        }
    }
}