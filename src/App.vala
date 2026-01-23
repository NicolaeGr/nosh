class App : Gtk.Application {
    static App instance;

    private TopBar.Bar bar;
    private Notifications.Window notifications;
    private QuickSettings.Window quick_settings;
    private AppLauncher.Window app_launcher;
    private Indicators.ChangeIndicator change_indicator;

    private void init_css() {
        var provider = new Gtk.CssProvider();
        provider.load_from_resource("/com/nicolaegr/nosh/style.css");

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
            if (argv.length >= 2) {
                var handler = Utils.RequestHandler.get_instance();
                bool handled = handler.handle_command(argv[1]);
                if (handled) {
                    return 0;
                }
            }

            command_line.print_literal("Unknown command or failed to handle command. \nIf you intended to start the main instance, it is already running.\n");
            return 0;
        }

        init_css();

        #if !STABLE_BUILD
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
        add_window((notifications = new Notifications.Window()));
        add_window((quick_settings = new QuickSettings.Window()));
        add_window((app_launcher = new AppLauncher.Window()));
        add_window((change_indicator = new Indicators.ChangeIndicator()));

        bar.present();

        #if !STABLE_BUILD
        bar.close_request.connect(() => {
            try {
                GLib.Process.spawn_command_line_async("systemctl --user start nosh");
            } catch (Error e) {
                warning("Failed to start stable nosh: %s\n", e.message);
            }
            return false;
        });
        #endif

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
