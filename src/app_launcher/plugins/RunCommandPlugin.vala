namespace AppLauncher {
    public class RunCommandPlugin : Object, Plugin {
        public bool matches(string query) {
            string lower = query.down();
            return lower.has_prefix("run:") || lower.has_prefix("kt:") || lower.has_prefix("kitty:");
        }
        
        public string get_display_label() {
            return "run: / kt: / kitty:";
        }
        
        public string get_icon() {
            return "utilities-terminal";
        }
        
        public string get_name() {
            return "Run Command";
        }
        
        public int get_priority() {
            return 100;
        }
        
        public AppEntry? create_entry(string query) {
            string lower = query.down();
            string command;
            bool use_kitty = false;
            
            if (lower.has_prefix("run:")) {
                command = query.substring(4).strip();
            } else if (lower.has_prefix("kitty:")) {
                command = query.substring(6).strip();
                use_kitty = true;
            } else if (lower.has_prefix("kt:")) {
                command = query.substring(3).strip();
                use_kitty = true;
            } else {
                return null;
            }
            
            if (command == "") {
                return null;
            }
            
            var entry = new AppEntry(
                use_kitty ? "Kitty: " + command : "Run: " + command,
                get_icon()
            );
            
            if (use_kitty) {
                entry.exec_command = "kitty -e " + command;
                entry.description = "Execute command in Kitty terminal";
            } else {
                entry.exec_command = command;
                entry.description = "Execute command";
            }
            
            return entry;
        }
    }
}
