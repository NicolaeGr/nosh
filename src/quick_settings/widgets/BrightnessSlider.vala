using Gtk;

namespace QuickSettings.Widgets {
    public class BrightnessSlider : Gtk.Box {
        private Gtk.Scale scale;
        private Gtk.Label percentage_label;
        private bool updating = false;

        public BrightnessSlider () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 2);
            set_css_classes ({"slider"});
            margin_top = 6;
            margin_bottom = 6;
            margin_start = 12;
            margin_end = 12;

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.halign = Gtk.Align.FILL;

            var icon = new Gtk.Image.from_icon_name ("display-brightness-symbolic");
            var label = new Gtk.Label ("Brightness");
            percentage_label = new Gtk.Label ("");
            percentage_label.set_css_classes ({"numeric"});
            percentage_label.halign = Gtk.Align.END;
            percentage_label.hexpand = true;

            header.append (icon);
            header.append (label);
            header.append (percentage_label);

            scale = new Gtk.Scale.with_range (Gtk.Orientation.HORIZONTAL, 0.0, 100.0, 1.0);
            scale.draw_value = false;
            scale.hexpand = true;

            append (header);
            append (scale);

            get_brightness.begin ();

            scale.value_changed.connect (() => {
                if (!updating) {
                    set_brightness ((int)scale.get_value ());
                }
                update_percentage ();
            });

            update_percentage ();
        }

        private async void get_brightness () {
            try {
                string stdout, stderr;
                int exit_code;
                string[] argv = {"light", "-G"};
                
                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                    null, out stdout, out stderr, out exit_code);
                
                if (exit_code == 0) {
                    var brightness = (double)int.parse (stdout.strip ());
                    updating = true;
                    scale.set_value (brightness);
                    updating = false;
                    update_percentage ();
                }
            } catch (Error e) {
                GLib.warning ("Error getting brightness: %s", e.message);
            }
        }

        private void set_brightness (int value) {
            Thread<bool> thread = new Thread<bool> ("brightness-setter", () => {
                try {
                    string[] argv = {"light", "-S", value.to_string ()};
                    string stdout, stderr;
                    int exit_code;
                    
                    Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                        null, out stdout, out stderr, out exit_code);
                    
                    if (exit_code == 0) {
                        return true;
                    } else {
                        GLib.warning ("Error setting brightness: %s", stderr);
                        return false;
                    }
                } catch (Error e) {
                    GLib.warning ("Error executing light command: %s", e.message);
                    return false;
                }
            });

            thread.join ();
        }

        private void update_percentage () {
            var percent = (int)scale.get_value ();
            percentage_label.set_label (@"$percent%");
        }
    }
}
