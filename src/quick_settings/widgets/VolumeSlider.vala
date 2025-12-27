using Gtk;

namespace QuickSettings.Widgets {
    public class VolumeSlider : Gtk.Box {
        private AstalWp.Audio audio;
        private Gtk.Scale scale;
        private Gtk.Label percentage_label;

        public VolumeSlider () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 2);
            set_css_classes ({"slider", "volume"});
            margin_top = 6;
            margin_bottom = 6;
            margin_start = 12;
            margin_end = 12;

            audio = AstalWp.get_default ().audio;

            if (audio.default_speaker == null) {
                return;
            }

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.halign = Gtk.Align.FILL;

            var icon = new Gtk.Image.from_icon_name ("audio-volume-medium-symbolic");
            var label = new Gtk.Label ("Volume");
            percentage_label = new Gtk.Label ("");
            percentage_label.set_css_classes ({"numeric"});
            percentage_label.halign = Gtk.Align.END;
            percentage_label.hexpand = true;

            header.append (icon);
            header.append (label);
            header.append (percentage_label);

            scale = new Gtk.Scale.with_range (Gtk.Orientation.HORIZONTAL, 0.0, 150.0, 1.0);
            scale.draw_value = false;
            scale.hexpand = true;
            scale.set_css_classes ({"volume-scale"});

            append (header);
            append (scale);

            var speaker = audio.default_speaker;
            scale.set_value (speaker.volume * 100);

            scale.value_changed.connect (() => {
                speaker.volume = scale.get_value () / 100.0;
                update_overamp_class ();
            });

            speaker.notify["volume"].connect (() => {
                scale.set_value (speaker.volume * 100);
                update_percentage ();
                update_overamp_class ();
            });

            update_percentage ();
            update_overamp_class ();
        }

        private void update_overamp_class () {
            var speaker = audio.default_speaker;
            if (speaker != null) {
                var percent = (int)(speaker.volume * 100);
                if (percent > 100) {
                    scale.add_css_class ("overamp");
                } else {
                    scale.remove_css_class ("overamp");
                }
            }
        }

        private void update_percentage () {
            var speaker = audio.default_speaker;
            if (speaker != null) {
                var percent = (int)(speaker.volume * 100);
                percentage_label.set_label (@"$percent%");
            }
        }
    }
}
