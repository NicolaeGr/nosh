namespace AppLauncher {
    public interface Plugin : Object {
        // Check if this plugin handles the given query
        public abstract bool matches(string query);
        
        // Get the display label for this plugin (e.g., "run:", "web:", "!g")
        public abstract string get_display_label();
        
        // Get the icon to display when this plugin is active
        public abstract string get_icon();
        
        // Get the display name for this plugin
        public abstract string get_name();
        
        // Create an AppEntry for this plugin's action
        public abstract AppEntry? create_entry(string query);
        
        // Priority for matching (higher = checked first)
        public abstract int get_priority();
    }
    
    public class PluginManager : Object {
        private static PluginManager? _instance = null;
        private Gee.ArrayList<Plugin> plugins;
        
        public signal void active_plugin_changed(Plugin? plugin);
        
        private PluginManager() {
            plugins = new Gee.ArrayList<Plugin>();
            register_default_plugins();
        }
        
        public static PluginManager get_instance() {
            if (_instance == null) {
                _instance = new PluginManager();
            }
            return _instance;
        }
        
        private void register_default_plugins() {
            register(new RunCommandPlugin());
            register(new RestartNoshPlugin());
        }
        
        public void register(Plugin plugin) {
            plugins.add(plugin);
            plugins.sort((a, b) => b.get_priority() - a.get_priority());
        }
        
        public Plugin? get_active_plugin(string query) {
            foreach (var plugin in plugins) {
                if (plugin.matches(query)) {
                    return plugin;
                }
            }
            return null;
        }
        
        public AppEntry? handle_query(string query) {
            var plugin = get_active_plugin(query);
            if (plugin != null) {
                return plugin.create_entry(query);
            }
            return null;
        }
    }
}
