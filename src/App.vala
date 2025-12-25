class App : Gtk.Application {
    static App instance;

    private TopBar.Bar bar;
    private QuickSettings.Window quick_settings;

    private void init_css() {
        var provider = new Gtk.CssProvider();
        provider.load_from_resource("/style.css");

        var display = Gdk.Display.get_default();
        if (display != null) {
            Gtk.StyleContext.add_provider_for_display(
                                                      display,
                                                      provider,
                                                      Gtk.STYLE_PROVIDER_PRIORITY_USER
            );
        }
    }

    public override int command_line(ApplicationCommandLine command_line) {
        var argv = command_line.get_arguments();

        if (command_line.is_remote) {
            // app is already running we can print to remote
            command_line.print_literal("hello from the main instance\n");

            // for example, we could toggle the visibility of the bar
            if (argv.length >= 3 && argv[1] == "toggle" && argv[2] == "bar") {
                bar.visible = !bar.visible;
            }
        } else {
            // main instance, initialize stuff here
            init_css();

            #if !STABLE_BUILD
            // If this is dev mode, stop the stable instance first
            try {
                GLib.Process.spawn_command_line_sync("systemctl --user stop nosh");
            } catch (Error e) {
                warning("Failed to stop stable nosh: %s\n", e.message);
            }
            #endif

            #if STABLE_BUILD
            bar = new TopBar.Bar(false);
            #else
            bar = new TopBar.Bar(true);
            #endif
            add_window(bar);
            add_window((quick_settings = new QuickSettings.Window()));

            bar.present();
            //  quick_settings.present();

            #if !STABLE_BUILD
            // When the app is closed, restart the stable instance if we're in dev mode
            bar.close_request.connect(() => {
                try {
                    GLib.Process.spawn_command_line_async("systemctl --user start nosh");
                } catch (Error e) {
                    warning("Failed to start stable nosh: %s\n", e.message);
                }
                return false;
            });
            #endif
        }

        return 0;
    }

    private App() {
        #if STABLE_BUILD
        application_id = "com.nicolaegr.nosh";
        #else
        application_id = "com.nicolaegr.nosh.dev";
        #endif
        
        flags = ApplicationFlags.HANDLES_COMMAND_LINE;
    }

    static int main(string[] argv) {
        App.instance = new App();
        Environment.set_prgname("nosh");
        return App.instance.run(argv);
    }
}
