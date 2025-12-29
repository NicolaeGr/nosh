namespace AppLauncher {
    public class ConfigManager : Object {
        private static ConfigManager? _instance = null;
        private Gee.ArrayList<AppEntry> custom_entries;
        
        private ConfigManager() {
            custom_entries = new Gee.ArrayList<AppEntry>();
            load_config();
        }
        
        public static ConfigManager get_instance() {
            if (_instance == null) {
                _instance = new ConfigManager();
            }
            return _instance;
        }
        
        private void load_config() {
            var config_dir = Environment.get_user_config_dir();
            var config_path = Path.build_filename(config_dir, "nosh", "app_launcher.toml");
            
            var file = File.new_for_path(config_path);
            if (!file.query_exists()) {
                return;
            }
            
            try {
                string contents;
                file.load_contents(null, out contents, null);
                parse_toml(contents);
            } catch (Error e) {
                warning("Failed to load config: %s", e.message);
            }
        }
        
        private void parse_toml(string contents) {
            // Simple TOML parser for [[entry]] sections
            string[] lines = contents.split("\n");
            string? current_name = null;
            string? current_icon = null;
            string? current_exec = null;
            
            foreach (var line in lines) {
                line = line.strip();
                
                if (line.has_prefix("[[entry]]")) {
                    // Save previous entry if exists
                    if (current_name != null && current_exec != null) {
                        add_toml_entry(current_name, current_icon, current_exec);
                    }
                    current_name = null;
                    current_icon = null;
                    current_exec = null;
                } else if (line.has_prefix("name")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_name = parts[1].strip().replace("\"", "");
                    }
                } else if (line.has_prefix("icon")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_icon = parts[1].strip().replace("\"", "");
                    }
                } else if (line.has_prefix("exec")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_exec = parts[1].strip().replace("\"", "");
                    }
                }
            }
            
            // Save last entry
            if (current_name != null && current_exec != null) {
                add_toml_entry(current_name, current_icon, current_exec);
            }
        }
        
        private void add_toml_entry(string name, string? icon, string exec) {
            var entry = new AppEntry(name, icon);
            entry.launch_func = () => {
                try {
                    Process.spawn_command_line_async(exec);
                } catch (Error e) {
                    warning("Failed to launch %s: %s", name, e.message);
                }
            };
            custom_entries.add(entry);
        }
        
        public void add_custom_entry(string name, string? icon, AppEntry.AppLaunchFunc func) {
            var entry = new AppEntry(name, icon);
            entry.launch_func = func;
            custom_entries.add(entry);
        }
        
        public Gee.ArrayList<AppEntry> get_custom_entries() {
            return custom_entries;
        }
    }
}
