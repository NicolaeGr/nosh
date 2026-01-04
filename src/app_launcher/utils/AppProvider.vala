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
                        // Handle ThemedIcon properly
                        if (icon is ThemedIcon) {
                            var themed = (ThemedIcon) icon;
                            var names = themed.get_names();
                            if (names.length > 0) {
                                icon_name = names[0];
                            }
                        } else {
                            icon_name = icon.to_string();
                        }
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
            
            var scored_results = new Gee.ArrayList<ScoredEntry>();
            var lower_query = query.down();
            
            foreach (var app in all_apps) {
                int score = fuzzy_match_score(app.name.down(), lower_query);
                if (score > 0) {
                    scored_results.add(new ScoredEntry(app, score));
                }
            }
            
            // Sort by score (higher is better), then by frequency
            scored_results.sort((a, b) => {
                if (a.score != b.score) {
                    return b.score - a.score;
                }
                return b.entry.frequency - a.entry.frequency;
            });
            
            // Extract just the entries
            var results = new Gee.ArrayList<AppEntry>();
            foreach (var scored in scored_results) {
                results.add(scored.entry);
            }
            
            return results;
        }
        
        private class ScoredEntry : Object {
            public AppEntry entry;
            public int score;
            
            public ScoredEntry(AppEntry entry, int score) {
                this.entry = entry;
                this.score = score;
            }
        }
        
        private int fuzzy_match_score(string text, string pattern) {
            int text_idx = 0;
            int pattern_idx = 0;
            int score = 0;
            int consecutive = 0;
            bool in_word = false;
            
            while (text_idx < text.length && pattern_idx < pattern.length) {
                if (text[text_idx] == pattern[pattern_idx]) {
                    // Bonus for matching at word start
                    if (text_idx == 0 || text[text_idx - 1] == ' ' || text[text_idx - 1] == '-') {
                        score += 10;
                        in_word = true;
                    }
                    
                    // Bonus for consecutive matches
                    consecutive++;
                    score += consecutive * 5;
                    
                    pattern_idx++;
                } else {
                    consecutive = 0;
                    in_word = false;
                }
                text_idx++;
            }
            
            // Return 0 if pattern wasn't fully matched
            if (pattern_idx < pattern.length) {
                return 0;
            }
            
            // Bonus for matching the entire pattern
            score += 20;
            
            return score;
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
