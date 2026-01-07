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
            if (exec_command != null) {
                try {
                    string expanded = exec_command.replace("$HOME", Environment.get_home_dir());
                    
                    string[]? argv = null;
                    Shell.parse_argv(expanded, out argv);
                    
                    if (argv != null) {
                        Pid child_pid;
                        Process.spawn_async(
                            null,
                            argv,
                            null,
                            SpawnFlags.SEARCH_PATH | 
                            SpawnFlags.DO_NOT_REAP_CHILD | 
                            SpawnFlags.STDOUT_TO_DEV_NULL | 
                            SpawnFlags.STDERR_TO_DEV_NULL,
                            null,
                            out child_pid
                        );
                        
                        ChildWatch.add(child_pid, (pid, status) => {
                            Process.close_pid(pid);
                        });
                    }
                } catch (ShellError e) {
                    critical("ShellError for %s: %s", name, e.message);
                } catch (SpawnError e) {
                    critical("SpawnError for %s: %s", name, e.message);
                } catch (Error e) {
                    critical("Error launching %s: %s", name, e.message);
                }
            } else if (launch_func != null) {
                try {
                    launch_func();
                } catch (Error e) {
                    critical("Failed to launch custom entry %s: %s", name, e.message);
                }
            } else if (app_info != null) {
                try {
                    app_info.launch(null, null);
                } catch (Error e) {
                    critical("Failed to launch %s: %s", name, e.message);
                }
            }
        }
    }
}
