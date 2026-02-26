namespace AppLauncher {
    public class ConfigManager : Object {
        private static ConfigManager? _instance = null;
        private Gee.ArrayList<AppEntry> custom_entries;
        private uint64 last_modified_time = 0;
        private string config_path;

        private ConfigManager() {
            custom_entries = new Gee.ArrayList<AppEntry> ();
            var config_dir = Environment.get_user_config_dir();
            var nosh_config_dir = Path.build_filename(config_dir, "nosh");
            config_path = Path.build_filename(nosh_config_dir, "app_launcher.toml");
            load_config();
        }

        public static ConfigManager get_instance() {
            if (_instance == null) {
                _instance = new ConfigManager();
            }
            return _instance;
        }

        private void load_config() {
            var file = File.new_for_path(config_path);
            if (!file.query_exists()) {
                var config_dir = Environment.get_user_config_dir();
                var nosh_config_dir = Path.build_filename(config_dir, "nosh");
                create_example_config(nosh_config_dir, config_path);
                return;
            }

            try {
                FileInfo info = file.query_info(FileAttribute.TIME_MODIFIED, FileQueryInfoFlags.NONE);
                uint64 modified_time = info.get_attribute_uint64(FileAttribute.TIME_MODIFIED);
                
                if (modified_time == last_modified_time && custom_entries.size > 0) {
                    return;
                }

                last_modified_time = modified_time;
                custom_entries.clear();

                uint8[] contents_bytes;
                file.load_contents(null, out contents_bytes, null);
                string contents = (string) contents_bytes;
                
                if (contents.has_prefix("# EXAMPLE_CONFIG")) {
                    return;
                }

                parse_toml(contents);
            } catch (Error e) {
                warning("Failed to load config: %s", e.message);
            }
        }

        private void create_example_config(string config_dir, string config_path) {
            var dir = File.new_for_path(config_dir);
            try {
                dir.make_directory_with_parents();
            } catch (Error e) {
                if (!(e is IOError.EXISTS)) {
                    warning("Failed to create config directory: %s", e.message);
                    return;
                }
            }
            
            string example_config = """# EXAMPLE_CONFIG
# This is an example configuration file for the app launcher.
# To use this file, remove or comment out the first line (# EXAMPLE_CONFIG).
#
# Each entry should follow this format:
# [[entry]]
# name = "App Name"
# icon = "icon-name"
# exec = "command to execute"

# Example: Custom script
# [[entry]]
# name = "My Custom Script"
# icon = "application-x-executable"
# exec = "$HOME/scripts/my-script.sh"

# Example: Application with arguments
# [[entry]]
# name = "Firefox (Work Profile)"
# icon = "firefox"
# exec = "firefox -P work"
""";

            try {
                var file = File.new_for_path(config_path);
                file.replace_contents(example_config.data, null, false, FileCreateFlags.NONE, null);
            } catch (Error e) {
                warning("Failed to create example config: %s", e.message);
            }
        }

        private void parse_toml(string contents) {
            string[] lines = contents.split("\n");
            string? current_name = null;
            string? current_icon = null;
            string? current_exec = null;

            foreach (var line in lines) {
                line = line.strip();

                if (line.has_prefix("[[entry]]")) {
                    if (current_name != null && current_exec != null) {
                        add_toml_entry(current_name, current_icon, current_exec);
                    }
                    current_name = null;
                    current_icon = null;
                    current_exec = null;
                } else if (line.has_prefix("name")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_name = parse_toml_string(parts[1].strip());
                    }
                } else if (line.has_prefix("icon")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_icon = parse_toml_string(parts[1].strip());
                    }
                } else if (line.has_prefix("exec")) {
                    var parts = line.split("=", 2);
                    if (parts.length == 2) {
                        current_exec = parse_toml_string(parts[1].strip());
                    }
                }
            }
            
            if (current_name != null && current_exec != null) {
                add_toml_entry(current_name, current_icon, current_exec);
            }
        }

        private string parse_toml_string(string value) {
            // Remove surrounding quotes (handles both " and ')
            string result = value.strip();
            if ((result.has_prefix("\"") && result.has_suffix("\"")) ||
                (result.has_prefix("'") && result.has_suffix("'"))) {
                result = result.substring(1, result.length - 2);
            }
            return result;
        }

        private void add_toml_entry(string name, string? icon, string exec) {
            var entry = new AppEntry(name, icon);
            entry.exec_command = exec;
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

        public void reload() {
            load_config();
        }
    }
}