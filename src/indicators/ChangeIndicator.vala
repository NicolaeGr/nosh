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
        icon_widget.set_from_icon_name ("audio-volume-medium-symbolic");
        var percent = volume / Utils.VolumeControl.MAX_VOLUME;
        progress_bar.set_fraction (percent.clamp (0, 1));
        value_label.set_label (@"$(Math.lround(volume * 100))%");
        show_indicator ();
    }

    private void show_brightness_indicator (double brightness) {
        icon_widget.set_from_icon_name ("display-brightness-symbolic");
        value_label.set_label (@"$(brightness)%");
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
