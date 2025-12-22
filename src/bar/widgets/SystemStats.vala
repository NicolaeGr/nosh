using Gtk;

namespace TopBar.Widgets {
    public class SystemStats : Gtk.Box {
        private Gtk.Label cpu_label;
        private Gtk.Label mem_label;
        private uint timeout_id = 0;

        public SystemStats () {
            Object (orientation: Gtk.Orientation.HORIZONTAL);
            set_css_classes ({ "SystemStats" });

            cpu_label = new Gtk.Label ("  0%");
            cpu_label.set_css_classes ({ "cpu-stat" });

            mem_label = new Gtk.Label ("  0%");
            mem_label.set_css_classes ({ "mem-stat" });

            append (cpu_label);
            append (mem_label);

            timeout_id = Timeout.add_seconds (1, update_stats);

            update_stats ();

            notify["parent"].connect (() => {
                if (get_parent () == null && timeout_id > 0) {
                    Source.remove (timeout_id);
                    timeout_id = 0;
                }
            });
        }

        private bool update_stats () {
            try {
                string[] argv = { "top", "-bn1" };
                string output;
                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH, null, out output);

                var lines = output.split ("\n");
                bool got_cpu = false;
                bool got_mem = false;

                foreach (var line in lines) {
                    if (!got_cpu && line.contains ("Cpu(s):")) {
                        parse_cpu_line (line);
                        got_cpu = true;
                    } else if (!got_mem && line.contains ("Mem")) {
                        parse_mem_line (line);
                        got_mem = true;
                    }

                    if (got_cpu && got_mem)
                        break;
                }
            } catch (Error e) {
                warning ("Error reading stats with top: %s", e.message);
            }
            return true; 
        }

        private void parse_cpu_line (string line) {
            try {
                var parts = line.split (",");

                foreach (var part in parts) {
                    if (part.contains (" id")) {
                        var idle_str = part
                             .replace ("id", "")
                             .replace ("%", "")
                             .strip ();

                        double idle = double.parse (idle_str);
                        uint cpu_percent = (uint) (100.0 - idle);

                        update_label (cpu_label, " ", cpu_percent);
                        return;
                    }
                }
            } catch (Error e) {
                warning ("Error parsing CPU line: %s", e.message);
            }
        }

        private void parse_mem_line (string line) {
            try {
                var parts = line.split (",");

                if (parts.length >= 3) {
                    var total_part = parts[0]
                         .split (":")[1]
                         .replace ("total", "")
                         .strip ();

                    var used_part = parts[2]
                         .replace ("used", "")
                         .strip ();

                    double total = double.parse (total_part);
                    double used = double.parse (used_part);

                    if (total > 0) {
                        uint mem_percent = (uint) ((used / total) * 100.0);
                        update_label (mem_label, " ", mem_percent);
                    }
                }
            } catch (Error e) {
                warning ("Error parsing memory line: %s", e.message);
            }
        }

        private void update_label (Gtk.Label label, string icon, uint percent) {
            label.remove_css_class ("level-low");
            label.remove_css_class ("level-medium");
            label.remove_css_class ("level-high");
            label.remove_css_class ("level-critical");

            string color_class;
            if (percent < 50) {
                color_class = "level-low";
            } else if (percent < 70) {
                color_class = "level-medium";
            } else if (percent < 90) {
                color_class = "level-high";
            } else {
                color_class = "level-critical";
            }

            label.set_label (@"$icon $percent%");
            label.add_css_class (color_class);
        }
    }
}
