namespace AppLauncher {
    public class AppEntry : Object {
        public string name { get; set; }
        public string? icon { get; set; }
        public string? desktop_id { get; set; }
        public string? description { get; set; }
        public string[] ? categories { get; set; }
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
            this.desktop_id = app_info != null? app_info.get_id() : null;
        }

        public void launch() {
            if (launch_func != null) {
                launch_func();
                return;
            }

            if (exec_command != null) {
                launch_command(exec_command);
                return;
            }

            if (app_info != null) {
                launch_desktop_app(app_info);
                return;
            }

            warning("AppEntry '%s' has no launch method defined", name);
        }

        private void launch_command(string command) {
            try {
                string expanded = command.replace("$HOME", Environment.get_home_dir());

                // Escape double quotes inside the Lua execution command string
                string lua_dispatch = "hl.dsp.exec_cmd(\"" + expanded + "\")";
                string[] argv = { "hyprctl", "dispatch", lua_dispatch };

                Process.spawn_async(
                                    null,
                                    argv,
                                    null,
                                    SpawnFlags.SEARCH_PATH
                                    | SpawnFlags.STDOUT_TO_DEV_NULL
                                    | SpawnFlags.STDERR_TO_DEV_NULL,
                                    null,
                                    null
                );
            } catch (Error e) {
                critical("Failed to launch command '%s': %s", command, e.message);
            }
        }

        private void launch_desktop_app(AppInfo app) {
            var command = app.get_commandline();
            if (command != null) {
                // Remove % placeholders from the .desktop Exec line
                try {
                    var regex = new Regex("%[fFuUdDnNvmick]");
                    command = regex.replace_literal(command, -1, 0, "");
                    launch_command(command.strip());
                } catch (Error e) {
                    critical("Regex error: %s", e.message);
                }
            } else {
                try {
                    app.launch(null, null);
                } catch (Error e) {
                    critical("Failed to launch desktop app '%s': %s", name, e.message);
                }
            }
        }
    }
}