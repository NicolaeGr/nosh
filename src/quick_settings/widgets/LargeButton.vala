using Gtk;

namespace QuickSettings.Widgets {
    public class LargeButton : Gtk.Box {
        public Gtk.Image icon_widget { get; private set; }
        public Gtk.Label status_label { get; private set; }
        public Gtk.Button button { get; private set; }
        private uint scroll_timeout = 0;
        private int scroll_position = 0;
        private string full_text = "";

        public LargeButton () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);

            button = new Gtk.Button ();
            button.set_css_classes ({"QuickSettings-large-button"});

            var button_content = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            button_content.halign = Gtk.Align.CENTER;
            button_content.valign = Gtk.Align.CENTER;
            button_content.margin_top = 8;
            button_content.margin_bottom = 8;
            button_content.margin_start = 8;
            button_content.margin_end = 8;

            icon_widget = new Gtk.Image ();
            icon_widget.set_icon_size (Gtk.IconSize.LARGE);
            icon_widget.set_css_classes ({"QuickSettings-icon"});

            status_label = new Gtk.Label ("");
            status_label.set_css_classes ({"QuickSettings-status"});
            status_label.set_wrap (false);
            status_label.set_ellipsize (Pango.EllipsizeMode.NONE);
            status_label.set_justify (Gtk.Justification.CENTER);
            status_label.set_max_width_chars (10);

            button_content.append (icon_widget);
            button_content.append (status_label);
            button.set_child (button_content);
            
            append (button);
        }

        public void set_icon_name (string name) {
            icon_widget.set_from_icon_name (name);
        }

        public void set_status (string text) {
            full_text = text;
            scroll_position = 0;
            
            // Stop any existing animation
            if (scroll_timeout != 0) {
                GLib.Source.remove (scroll_timeout);
                scroll_timeout = 0;
            }
            
            // If text is longer than 10 chars, start scrolling
            if (text.length > 10) {
                update_scroll_text ();
                scroll_timeout = GLib.Timeout.add (500, () => {
                    update_scroll_text ();
                    return true;
                });
            } else {
                status_label.set_label (text);
            }
        }

        private void update_scroll_text () {
            if (full_text.length <= 10) {
                status_label.set_label (full_text);
                return;
            }
            
            // Show 10 characters, scroll through the text
            int max_pos = full_text.length - 10;
            string visible = full_text.substring (scroll_position, 10);
            status_label.set_label (visible);
            
            scroll_position = (scroll_position + 1) % (full_text.length + 10);
            if (scroll_position > max_pos) {
                scroll_position = 0;
            }
        }

        public void set_icon_enabled (bool enabled) {
            if (enabled) {
                icon_widget.remove_css_class ("disabled");
                icon_widget.add_css_class ("enabled");
            } else {
                icon_widget.remove_css_class ("enabled");
                icon_widget.add_css_class ("disabled");
            }
        }
    }
}
