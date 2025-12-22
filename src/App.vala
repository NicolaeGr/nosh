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
            add_window((bar = new TopBar.Bar()));
            add_window((quick_settings = new QuickSettings.Window()));
        }

        return 0;
    }

    private App() {
        application_id = "com.nicolaegr.nosh";
        flags = ApplicationFlags.HANDLES_COMMAND_LINE;
    }

    static int main(string[] argv) {
        App.instance = new App();
        Environment.set_prgname("nosh");
        return App.instance.run(argv);
    }
}