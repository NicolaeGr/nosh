namespace AppLauncher {
    public class AppProvider : Object {
        private static AppProvider? _instance = null;
        private Gee.ArrayList<AppEntry> all_apps;
        private DatabaseManager db_manager;
        private ConfigManager config_manager;
        
        private AppProvider() {
            all_apps = new Gee.ArrayList<AppEntry>();
            db_manager = DatabaseManager.get_instance();
            config_manager = ConfigManager.get_instance();
            load_apps();
        }
        
        public static AppProvider get_instance() {
            if (_instance == null) {
                _instance = new AppProvider();
            }
            return _instance;
        }
        
        private void load_apps() {
            // Load system apps
            var apps = AppInfo.get_all();
            foreach (var app_info in apps) {
                if (app_info.should_show()) {
                    var name = app_info.get_display_name();
                    var icon = app_info.get_icon();
                    string? icon_name = null;
                    
                    if (icon != null) {
                        icon_name = icon.to_string();
                    }
                    
                    var entry = new AppEntry(name, icon_name, app_info);
                    
                    // Load frequency from database
                    if (entry.desktop_id != null) {
                        entry.frequency = db_manager.get_frequency(entry.desktop_id);
                    }
                    
                    all_apps.add(entry);
                }
            }
            
            // Add custom entries
            var custom = config_manager.get_custom_entries();
            foreach (var entry in custom) {
                all_apps.add(entry);
            }
            
            // Sort by frequency (most used first)
            all_apps.sort((a, b) => {
                return b.frequency - a.frequency;
            });
        }
        
        public Gee.ArrayList<AppEntry> search(string query) {
            if (query.strip() == "") {
                return all_apps;
            }
            
            var results = new Gee.ArrayList<AppEntry>();
            var lower_query = query.down();
            
            foreach (var app in all_apps) {
                if (fuzzy_match(app.name.down(), lower_query)) {
                    results.add(app);
                }
            }
            
            return results;
        }
        
        private bool fuzzy_match(string text, string pattern) {
            int text_idx = 0;
            int pattern_idx = 0;
            
            while (text_idx < text.length && pattern_idx < pattern.length) {
                if (text[text_idx] == pattern[pattern_idx]) {
                    pattern_idx++;
                }
                text_idx++;
            }
            
            return pattern_idx == pattern.length;
        }
        
        public void record_launch(AppEntry entry) {
            if (entry.desktop_id != null) {
                db_manager.increment_frequency(entry.desktop_id);
                entry.frequency++;
                
                // Re-sort apps by frequency
                all_apps.sort((a, b) => {
                    return b.frequency - a.frequency;
                });
            }
        }
        
        public Gee.ArrayList<AppEntry> get_all_apps() {
            return all_apps;
        }
    }
}
