namespace AppLauncher {
    public class RestartNoshPlugin : Object, Plugin {
        public bool matches(string query) {
            string lower = query.down();
            return lower.contains("restart") && lower.contains("nosh");
        }
        
        public string get_display_label() {
            return "Restart Nosh";
        }
        
        public string get_icon() {
            return "system-restart";
        }
        
        public string get_name() {
            return "Restart Nosh";
        }
        
        public int get_priority() {
            return 200;
        }
        
        public AppEntry? create_entry(string query) {
            string lower = query.down();
            if (!matches(lower)) {
                return null;
            }
            
            var entry = new AppEntry(
                "Restart Nosh",
                get_icon(),
                null,
                () => {
                    restart_nosh();
                }
            );
            
            return entry;
        }
        
        private void restart_nosh() {
            try {
                GLib.Process.spawn_command_line_async("systemctl --user restart nosh");
            } catch (Error e) {
                warning("Failed to restart nosh: %s", e.message);
                try {
                    GLib.Process.spawn_command_line_async("nosh &");
                } catch (Error e2) {
                    warning("Failed to start nosh: %s", e2.message);
                }
            }
        }
    }
}
