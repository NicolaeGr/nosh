using Gtk;

namespace Widgets {
    public class AudioSlider : Gtk.Button {
        Gtk.Image icon = new Gtk.Image ();

        public AudioSlider () {
            set_child(icon);
            set_css_classes ({"square-icon" });
            set_valign (Align.CENTER);

            var speaker = AstalWp.get_default ().audio.default_speaker;
            speaker.bind_property ("volume-icon", icon, "icon-name", BindingFlags.SYNC_CREATE);

            var scroll_controller = new Gtk.EventControllerScroll (Gtk.EventControllerScrollFlags.VERTICAL);
            scroll_controller.scroll.connect ((dx, dy) => {
                var delta = dy > 0 ? -0.05 : 0.05;
                speaker.volume = double.max (0.0, double.min (1.0, speaker.volume + delta));
                return true;
            });
            add_controller (scroll_controller);

            speaker.notify["volume"].connect (() => {
                var vol_percent = Math.floor (speaker.volume * 100);
                set_tooltip_text (@"Volume: $vol_percent%");
            });
        }
    }
}