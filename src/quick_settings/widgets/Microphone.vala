using Gtk;

namespace QuickSettings.Widgets {
    public class Microphone : Gtk.Box {
        private AstalWp.Audio audio;
        private Gtk.Scale scale;
        private Gtk.Label percentage_label;

        public Microphone () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 2);
            set_css_classes ({"QuickSettings-slider"});
            margin_top = 6;
            margin_bottom = 6;
            margin_start = 12;
            margin_end = 12;

            audio = AstalWp.get_default ().audio;

            if (audio.default_microphone == null) {
                return;
            }

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.halign = Gtk.Align.FILL;

            var icon = new Gtk.Image.from_icon_name ("audio-input-microphone-symbolic");
            var label = new Gtk.Label ("Microphone");
            percentage_label = new Gtk.Label ("");
            percentage_label.set_css_classes ({"numeric"});
            percentage_label.halign = Gtk.Align.END;
            percentage_label.hexpand = true;

            header.append (icon);
            header.append (label);
            header.append (percentage_label);

            scale = new Gtk.Scale.with_range (Gtk.Orientation.HORIZONTAL, 0.0, 100.0, 1.0);
            scale.draw_value = false;
            scale.hexpand = true;

            append (header);
            append (scale);

            var mic = audio.default_microphone;
            scale.set_value (mic.volume * 100);

            scale.value_changed.connect (() => {
                mic.volume = scale.get_value () / 100.0;
            });

            mic.notify["volume"].connect (() => {
                scale.set_value (mic.volume * 100);
                update_percentage ();
            });

            update_percentage ();
        }

        private void update_percentage () {
            var mic = audio.default_microphone;
            if (mic != null) {
                var percent = (int)(mic.volume * 100);
                percentage_label.set_label (@"$percent%");
            }
        }
    }
}
