namespace AppLauncher {
    public class AppEntry : Object {
        public string name { get; set; }
        public string? icon { get; set; }
        public string? desktop_id { get; set; }
        public string? description { get; set; }
        public string[]? categories { get; set; }
        public AppInfo? app_info { get; set; }
        public AppLaunchFunc? launch_func { get; set; }
        public string? exec_command { get; set; }
        public int frequency { get; set; default = 0; }
        
        public delegate void AppLaunchFunc();
        
        public AppEntry(string name, string? icon = null, AppInfo? app_info = null, AppLaunchFunc? launch_func = null) {
            this.name = name;
            this.icon = icon;
            this.app_info = app_info;
            this.launch_func = launch_func;
            this.desktop_id = app_info != null ? app_info.get_id() : null;
        }
        
        public void launch() {
            if (launch_func != null) {
                try {
                    launch_func();
                } catch (Error e) {
                    critical("Failed to launch custom entry %s: %s", name, e.message);
                }
                return;
            }
            
            string? command = null;
            
            if (exec_command != null) {
                command = exec_command.replace("$HOME", Environment.get_home_dir());
            } else if (app_info != null) {
                command = app_info.get_commandline();
            }
            
            if (command == null) {
                if (app_info != null) {
                    try {
                        app_info.launch(null, null);
                    } catch (Error e) {
                        critical("Failed to launch %s: %s", name, e.message);
                    }
                }
                return;
            }
            
            try {
                string[] argv = { 
                    "sh", "-c", 
                    "setsid -f sh -c '" + command.replace("'", "'\\''") + "' >/dev/null 2>&1 </dev/null" 
                };
                
                Process.spawn_async(null, argv, null, SpawnFlags.SEARCH_PATH, null, null);
            } catch (Error e) {
                critical("Failed to launch %s: %s", name, e.message);
            }
        }
    }
}
