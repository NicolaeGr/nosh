namespace AppLauncher.Widgets {
    public class AppGrid : Gtk.Box {
        private Gtk.ScrolledWindow scrolled_window;
        private Gtk.FlowBox flow_box;
        private Gee.ArrayList<AppEntry> apps;
        public int selected_index = 0;
        private const int COLUMNS = 2;
        
        public signal void app_activated(AppEntry entry);
        
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
            flow_box.set_max_children_per_line(COLUMNS);
            flow_box.set_min_children_per_line(COLUMNS);
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
        
        // Navigation methods called from SearchBar
        public void move_up() {
            if (apps.size == 0) return;
            
            int new_index = selected_index - COLUMNS;
            if (new_index >= 0) {
                selected_index = new_index;
                update_selection();
            }
        }
        
        public void move_down() {
            if (apps.size == 0) return;
            
            int new_index = selected_index + COLUMNS;
            if (new_index < apps.size) {
                selected_index = new_index;
                update_selection();
            }
        }
        
        public void move_left() {
            if (apps.size == 0) return;
            
            print("AppGrid: move_left called, selected_index=%d, column=%d\n", selected_index, selected_index % COLUMNS);
            
            // Don't move if already at leftmost column
            if (selected_index % COLUMNS != 0) {
                selected_index--;
                update_selection();
            }
        }
        
        public void move_right() {
            if (apps.size == 0) return;
            
            print("AppGrid: move_right called, selected_index=%d, column=%d, apps.size=%d\n", 
                  selected_index, selected_index % COLUMNS, apps.size);
            
            // Don't move if already at rightmost column or last item
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
                // Get the adjustment from scrolled window
                var adj = scrolled_window.get_vadjustment();
                double child_y;
                child.translate_coordinates(flow_box, 0, 0, null, out child_y);
                
                // Scroll to make the selected item visible
                if (child_y < adj.get_value()) {
                    adj.set_value(child_y);
                } else if (child_y + child.get_height() > adj.get_value() + adj.get_page_size()) {
                    adj.set_value(child_y + child.get_height() - adj.get_page_size());
                }
            }
        }
    }
}
