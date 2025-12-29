namespace AppLauncher.Widgets {
    public class AppGrid : Gtk.ScrolledWindow {
        private Gtk.FlowBox flow_box;
        private Gee.ArrayList<AppEntry> apps;
        private int selected_index = 0;
        
        public signal void app_activated(AppEntry entry);
        
        public AppGrid() {
            Object(
                hscrollbar_policy: Gtk.PolicyType.NEVER,
                vscrollbar_policy: Gtk.PolicyType.AUTOMATIC,
                vexpand: true,
                hexpand: true
            );
            
            set_css_classes({"AppGrid"});
            
            flow_box = new Gtk.FlowBox();
            flow_box.set_valign(Gtk.Align.START);
            flow_box.set_max_children_per_line(2);
            flow_box.set_min_children_per_line(2);
            flow_box.set_column_spacing(8);
            flow_box.set_row_spacing(8);
            flow_box.set_homogeneous(true);
            flow_box.set_selection_mode(Gtk.SelectionMode.SINGLE);
            
            set_child(flow_box);
            
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
            
            // Clear existing children
            var child = flow_box.get_first_child();
            while (child != null) {
                var next = child.get_next_sibling();
                flow_box.remove(child);
                child = next;
            }
            
            // Add new apps
            foreach (var app in apps) {
                var item = create_app_item(app);
                flow_box.append(item);
            }
            
            // Select first item
            if (apps.size > 0) {
                var first_child = flow_box.get_child_at_index(0);
                if (first_child != null) {
                    flow_box.select_child(first_child);
                }
            }
        }
        
        private Gtk.Box create_app_item(AppEntry app) {
            var box = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 8);
            box.set_css_classes({"AppItem"});
            
            // Icon
            var icon_widget = new Gtk.Image();
            if (app.icon != null) {
                icon_widget.set_from_icon_name(app.icon);
                icon_widget.set_pixel_size(32);
            } else {
                icon_widget.set_from_icon_name("application-x-executable");
                icon_widget.set_pixel_size(32);
            }
            box.append(icon_widget);
            
            // Name
            var label = new Gtk.Label(app.name);
            label.set_ellipsize(Pango.EllipsizeMode.END);
            label.set_xalign(0);
            label.set_hexpand(true);
            box.append(label);
            
            return box;
        }
        
        public void select_next() {
            if (apps.size == 0) return;
            
            selected_index = (selected_index + 1) % apps.size;
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                flow_box.select_child(child);
                // Scroll to make visible
                var adjustment = get_vadjustment();
                var allocation = Graphene.Rect();
                child.compute_bounds(flow_box, out allocation);
                adjustment.set_value(allocation.get_y());
            }
        }
        
        public void select_previous() {
            if (apps.size == 0) return;
            
            selected_index = selected_index - 1;
            if (selected_index < 0) {
                selected_index = apps.size - 1;
            }
            
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                flow_box.select_child(child);
                // Scroll to make visible
                var adjustment = get_vadjustment();
                var allocation = Graphene.Rect();
                child.compute_bounds(flow_box, out allocation);
                adjustment.set_value(allocation.get_y());
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
    }
}
