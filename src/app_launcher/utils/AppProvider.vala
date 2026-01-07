namespace AppLauncher {
    public class AppProvider : Object {
        private static AppProvider? _instance = null;
        private Gee.ArrayList<AppEntry> all_apps;
        private DatabaseManager db_manager;
        private ConfigManager config_manager;
        private PluginManager plugin_manager;
        
        private AppProvider() {
            all_apps = new Gee.ArrayList<AppEntry>();
            db_manager = DatabaseManager.get_instance();
            config_manager = ConfigManager.get_instance();
            plugin_manager = PluginManager.get_instance();
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
                    
                    // Load description and categories
                    entry.description = app_info.get_description();
                    
                    // Get categories from desktop file
                    var desktop_app = app_info as GLib.DesktopAppInfo;
                    if (desktop_app != null) {
                        var categories_str = desktop_app.get_categories();
                        if (categories_str != null) {
                            entry.categories = categories_str.split(";");
                        }
                    }
                    
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
            
            // Check if a plugin handles this query
            var plugin_entry = plugin_manager.handle_query(query);
            if (plugin_entry != null) {
                var results = new Gee.ArrayList<AppEntry>();
                results.add(plugin_entry);
                return results;
            }
            
            var scored_results = new Gee.ArrayList<ScoredEntry>();
            var lower_query = query.down();
            
            foreach (var app in all_apps) {
                int score = fuzzy_match_score(app.name.down(), lower_query);
                
                // Also search in description
                if (score == 0 && app.description != null) {
                    score = fuzzy_match_score(app.description.down(), lower_query) / 2; // Lower priority
                }
                
                // Also search in categories
                if (score == 0 && app.categories != null) {
                    foreach (var category in app.categories) {
                        int cat_score = fuzzy_match_score(category.down(), lower_query) / 3; // Even lower priority
                        if (cat_score > score) {
                            score = cat_score;
                        }
                    }
                }
                
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
            
            // If no results found and query looks like a command, add a "Run command" option
            if (results.size == 0 && is_valid_command(query)) {
                var cmd_entry = create_command_entry(query);
                results.add(cmd_entry);
            }
            
            return results;
        }
        
        private bool is_valid_command(string query) {
            // Don't allow empty or whitespace-only queries
            if (query.strip() == "") {
                return false;
            }
            
            // Extract the command (first word)
            string[] parts = query.strip().split(" ");
            if (parts.length == 0) {
                return false;
            }
            
            string command = parts[0];
            
            // Check if command contains path separator (likely a path)
            if (command.contains("/")) {
                return true;
            }
            
            // Check if command exists in PATH
            string? path = Environment.find_program_in_path(command);
            return path != null;
        }
        
        private AppEntry create_command_entry(string command) {
            var entry = new AppEntry("Run: " + command, "utilities-terminal");
            entry.exec_command = command;
            return entry;
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
            // First, check if it's a simple substring match (most common case)
            if (text.contains(pattern)) {
                int pos = text.index_of(pattern);
                int score = 1000; // High base score for substring match
                
                // Bonus for matching at the start
                if (pos == 0) {
                    score += 500;
                } else if (pos > 0 && (text[pos - 1] == ' ' || text[pos - 1] == '-' || text[pos - 1] == '_')) {
                    // Bonus for matching at word boundary
                    score += 200;
                }
                
                return score;
            }
            
            // Fallback to fuzzy matching for more complex cases
            int text_idx = 0;
            int pattern_idx = 0;
            int score = 0;
            int consecutive = 0;
            int max_consecutive = 0;
            int matched_at_start = 0;
            int gaps = 0;
            
            while (text_idx < text.length && pattern_idx < pattern.length) {
                if (text[text_idx] == pattern[pattern_idx]) {
                    // Track consecutive matches
                    consecutive++;
                    if (consecutive > max_consecutive) {
                        max_consecutive = consecutive;
                    }
                    
                    // Strong bonus for matching at the very beginning
                    if (text_idx == 0 && pattern_idx == 0) {
                        score += 100;
                        matched_at_start++;
                    }
                    
                    // Bonus for matching at word start
                    if (text_idx > 0 && (text[text_idx - 1] == ' ' || text[text_idx - 1] == '-' || text[text_idx - 1] == '_')) {
                        score += 30;
                    }
                    
                    // Strong bonus for consecutive matches
                    if (consecutive > 1) {
                        score += consecutive * 20;
                    } else {
                        score += 10;
                    }
                    
                    // Track continuous matches from start
                    if (text_idx == pattern_idx) {
                        matched_at_start++;
                    }
                    
                    pattern_idx++;
                } else {
                    if (consecutive > 0) {
                        gaps++;
                    }
                    consecutive = 0;
                }
                text_idx++;
            }
            
            // Return 0 if pattern wasn't fully matched
            if (pattern_idx < pattern.length) {
                return 0;
            }
            
            // Reject if we have too many gaps - this prevents "reboot" from matching "ebook"
            // Require that at least 60% of the pattern is in consecutive chunks
            if (max_consecutive < pattern.length * 0.6) {
                return 0;
            }
            
            // Penalize heavily for scattered matches (many gaps)
            if (gaps > 2) {
                score = score / (gaps * 2);
            }
            
            // Require a minimum score threshold
            if (score < 30) {
                return 0;
            }
            
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
        
        public void reload() {
            all_apps.clear();
            config_manager.reload();
            load_apps();
        }
    }
}
