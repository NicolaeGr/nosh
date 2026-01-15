using Gtk;

[GtkTemplate (ui = "/com/nicolaegr/nosh/indicators/ChangeIndicator.ui")]
class Indicators.ChangeIndicator: Astal.Window {
    [GtkChild]
    private Gtk.Image icon_widget;

    [GtkChild]
    private Gtk.ProgressBar progress_bar;

    [GtkChild]
    private Gtk.Label value_label;

    private Utils.SystemControlManager control_manager;
    private uint hide_timer = 0;
    private const uint HIDE_TIMEOUT = 800;

    public ChangeIndicator () {
        Object ();

        control_manager = Utils.SystemControlManager.get_instance ();

        control_manager.volume.volume_changed.connect ((volume, old_vol) => {
            show_volume_indicator (volume);
        });

        control_manager.brightness.brightness_changed.connect ((brightness, old_bright) => {
            show_brightness_indicator (brightness);
        });
    }

    private void show_volume_indicator (double volume) {
        var percent = volume / Utils.VolumeControl.MAX_VOLUME;
        var percentage_value = Math.lround(volume * 100);
        show_indicator_with_range (
            "audio-volume-medium-symbolic",
            @"$(percentage_value)%",
            percent,
            percentage_value > 100
        );
    }

    private void show_brightness_indicator (double brightness) {
        var percent = brightness / 100.0;
        show_indicator_with_range (
            "display-brightness-symbolic",
            @"$(brightness)%",
            percent
        );
    }

    private void show_indicator_with_range (string icon, string label, double fraction, bool overamplify = false) {
        icon_widget.set_from_icon_name (icon);
        value_label.set_label (label);
        progress_bar.set_fraction (fraction.clamp (0, 1));
        
        if (overamplify) {
            progress_bar.add_css_class ("overamp");
        } else {
            progress_bar.remove_css_class ("overamp");
        }
        
        show_indicator ();
    }

    private void show_indicator () {
        visible = true;

        if (hide_timer != 0) {
            GLib.Source.remove (hide_timer);
        }

        hide_timer = GLib.Timeout.add (HIDE_TIMEOUT, () => {
            visible = false;
            hide_timer = 0;
            return false;
        });
    }
}
