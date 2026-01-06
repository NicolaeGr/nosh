namespace AppLauncher.Widgets {
    public class AppGrid : Gtk.Box {
        private Gtk.ScrolledWindow scrolled_window;
        private Gtk.FlowBox flow_box;
        private Gee.ArrayList<AppEntry> apps;
        private int selected_index = 0;
        
        public signal void app_activated(AppEntry entry);
        public signal void move_up_to_search();
        
        public AppGrid() {
            Object(
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 0,
                vexpand: true,
                hexpand: true
            );
            
            set_css_classes({"AppGrid"});
            
            // Create scrolled window
            scrolled_window = new Gtk.ScrolledWindow();
            scrolled_window.hscrollbar_policy = Gtk.PolicyType.NEVER;
            scrolled_window.vscrollbar_policy = Gtk.PolicyType.AUTOMATIC;
            scrolled_window.vexpand = true;
            scrolled_window.hexpand = true;
            
            flow_box = new Gtk.FlowBox();
            flow_box.set_valign(Gtk.Align.START);
            flow_box.set_max_children_per_line(2);
            flow_box.set_min_children_per_line(2);
            flow_box.set_column_spacing(8);
            flow_box.set_row_spacing(8);
            flow_box.set_homogeneous(true);
            flow_box.set_selection_mode(Gtk.SelectionMode.SINGLE);
            
            scrolled_window.set_child(flow_box);
            append(scrolled_window);
            
            flow_box.child_activated.connect((child) => {
                var index = child.get_index();
                if (index >= 0 && index < apps.size) {
                    activate_app(apps[index]);
                }
            });
            
            // Add key controller for arrow navigation
            var key_controller = new Gtk.EventControllerKey();
            key_controller.key_pressed.connect(on_key_pressed);
            flow_box.add_controller(key_controller);
        }
        
        private bool on_key_pressed(uint keyval, uint keycode, Gdk.ModifierType state) {
            if (keyval == Gdk.Key.Down) {
                select_next_row();
                return true;
            } else if (keyval == Gdk.Key.Up) {
                if (selected_index < 2) {
                    // If on first row, move back to search
                    move_up_to_search();
                    return true;
                }
                select_previous_row();
                return true;
            } else if (keyval == Gdk.Key.Left) {
                select_previous();
                return true;
            } else if (keyval == Gdk.Key.Right) {
                select_next();
                return true;
            }
            return false;
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
            }
        }
        
        public void select_next_row() {
            if (apps.size == 0) return;
            
            // Move down by 2 (number of columns)
            selected_index = (selected_index + 2) % apps.size;
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                flow_box.select_child(child);
            }
        }
        
        public void select_previous_row() {
            if (apps.size == 0) return;
            
            // Move up by 2 (number of columns)
            selected_index = selected_index - 2;
            if (selected_index < 0) {
                selected_index = 0;
            }
            
            var child = flow_box.get_child_at_index(selected_index);
            if (child != null) {
                flow_box.select_child(child);
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
