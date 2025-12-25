using Gtk;

namespace QuickSettings.Widgets {
    public class Card : Gtk.Box {
        public Card () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            set_css_classes ({"QuickSettings-card"});
            halign = Gtk.Align.END;
            valign = Gtk.Align.START;
            vexpand = false;
            margin_top = 56;
            margin_end = 16;
            margin_bottom = 16;

            var scroll = new Gtk.ScrolledWindow ();
            scroll.hexpand = false;
            scroll.vexpand = false;
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.NEVER);
            scroll.set_min_content_width (260);

            var content = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            
            content.append (new Connectivity ());

            var separator1 = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator1.set_css_classes ({"QuickSettings-divider"});
            content.append (separator1);

            var sliders_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            sliders_box.append (new VolumeSlider ());
            sliders_box.append (new Microphone ());
            sliders_box.append (new BrightnessSlider ());
            content.append (sliders_box);

            var separator2 = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator2.set_css_classes ({"QuickSettings-divider"});
            content.append (separator2);

            var toggles_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            toggles_box.set_css_classes ({"QuickSettings-item-row", "QuickSettings-toggles"});
            toggles_box.halign = Gtk.Align.FILL;
            toggles_box.homogeneous = true;

            var dnd_button = new QuickToggleButton ("dnd", "notifications-disabled-symbolic", "DND");
            var sleep_inhibitor = new QuickToggleButton ("sleep", "eye-slash-symbolic", "No Sleep");
            var vpn_button = new QuickToggleButton ("vpn", "security-high-symbolic", "VPN");

            toggles_box.append (dnd_button);
            toggles_box.append (sleep_inhibitor);
            toggles_box.append (vpn_button);

            content.append (toggles_box);

            scroll.set_child (content);
            append (scroll);
        }
    }
}
