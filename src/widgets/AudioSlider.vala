namespace Widgets {
    public class AudioSlider : Gtk.Box {
        Gtk.Image icon = new Gtk.Image ();
        Astal.Slider slider = new Astal.Slider () { hexpand = true };

        public AudioSlider () {
            append (icon);
            append (slider);
            set_css_classes ({ "AudioSlider" });
            // Astal.widget_set_css (this, "min-width: 140px");

            var speaker = AstalWp.get_default ().audio.default_speaker;
            speaker.bind_property ("volume-icon", icon, "icon-name", BindingFlags.SYNC_CREATE);
            speaker.bind_property ("volume", slider, "value", BindingFlags.SYNC_CREATE);
            slider.value_changed.connect (() => {
                speaker.volume = slider.value;
            });
        }
    }
}