using Gtk;

namespace Indicators {
    public class ChangeIndicatorWindow : Astal.Window {
        private Gtk.Box card_box;
        private Gtk.Image icon_widget;
        private Gtk.ProgressBar progress_bar;
        private Gtk.Label value_label;
        private Utils.ChangeIndicatorManager indicator_manager;
        private uint hide_timer = 0;
        private const uint HIDE_TIMEOUT = 800;

        public ChangeIndicatorWindow () {
            Object (
                anchor: Astal.WindowAnchor.BOTTOM
                | Astal.WindowAnchor.LEFT
                | Astal.WindowAnchor.RIGHT,
                exclusivity: Astal.Exclusivity.IGNORE,
                layer: Astal.Layer.TOP,
                keymode: Astal.Keymode.NONE,
                visible: false,
                namespace: "nosh-change-indicator"
            );

            set_css_classes ({"ChangeIndicator"});

            indicator_manager = Utils.ChangeIndicatorManager.get_instance ();

            card_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 12);
            card_box.set_css_classes ({"indicator-card"});
            card_box.halign = Gtk.Align.CENTER;
            card_box.valign = Gtk.Align.END;
            card_box.margin_bottom = 40;

            var top_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            top_box.halign = Gtk.Align.CENTER;

            icon_widget = new Gtk.Image ();
            icon_widget.set_css_classes ({"indicator-icon"});
            icon_widget.halign = Gtk.Align.CENTER;

            value_label = new Gtk.Label ("");
            value_label.set_css_classes ({"indicator-value"});
            value_label.halign = Gtk.Align.CENTER;

            top_box.append (icon_widget);
            top_box.append (value_label);

            progress_bar = new Gtk.ProgressBar ();
            progress_bar.set_css_classes ({"indicator-bar"});
            progress_bar.hexpand = true;

            card_box.append (top_box);
            card_box.append (progress_bar);

            set_child (card_box);

            indicator_manager.volume_changed.connect ((volume, old_vol) => {
                show_volume_indicator (volume);
            });

            indicator_manager.brightness_changed.connect ((brightness, old_bright) => {
                show_brightness_indicator (brightness);
            });
        }

        private void show_volume_indicator (double volume) {
            icon_widget.set_from_icon_name ("audio-volume-medium-symbolic");
            var percent = volume / Utils.ChangeIndicatorManager.MAX_VOLUME;
            progress_bar.set_fraction (percent.clamp (0, 1));
            value_label.set_label (@"$(Math.lround(volume * 100))%");
            show_indicator ();
        }

        private void show_brightness_indicator (double brightness) {
            icon_widget.set_from_icon_name ("display-brightness-symbolic");
            var percent = brightness / Utils.ChangeIndicatorManager.MAX_BRIGHTNESS;
            progress_bar.set_fraction (percent.clamp (0, 1));
            value_label.set_label (@"$(Math.lround(brightness))%");
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
}
