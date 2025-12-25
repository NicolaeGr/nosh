using Gtk;

namespace QuickSettings.Widgets {
    public class KDEConnectDropdown : Gtk.Box {
        private Gtk.Box content_box;
        private Gtk.ScrolledWindow scroll;
        private Gtk.Switch kde_switch;

        public KDEConnectDropdown () {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            set_css_classes ({"QuickSettings-dropdown"});
            hexpand = true;
            vexpand = false;

            // Header with switch and collapse button
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"QuickSettings-dropdown-header"});
            header.halign = Gtk.Align.FILL;
            header.margin_start = 8;
            header.margin_end = 8;
            header.margin_top = 8;
            header.margin_bottom = 8;

            var header_label = new Gtk.Label ("KDE Connect");
            header_label.halign = Gtk.Align.START;

            kde_switch = new Gtk.Switch ();
            kde_switch.set_css_classes ({"small-switch"});
            kde_switch.active = true; // TODO: Connect to actual KDE Connect state
            kde_switch.halign = Gtk.Align.START;
            kde_switch.valign = Gtk.Align.CENTER;
            kde_switch.hexpand = true;

            // Refresh button
            var refresh_button = new Gtk.Button.from_icon_name ("view-refresh-symbolic");
            refresh_button.halign = Gtk.Align.END;
            refresh_button.set_size_request (24, 24);
            refresh_button.clicked.connect (() => {
                update_devices ();
            });

            var collapse_button = new Gtk.Button.from_icon_name ("go-up-symbolic");
            collapse_button.halign = Gtk.Align.END;
            collapse_button.set_size_request (24, 24);

            header.append (header_label);
            header.append (kde_switch);
            header.append (refresh_button);
            header.append (collapse_button);

            append (header);

            // Separator
            var separator = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            separator.set_css_classes ({"QuickSettings-divider"});
            append (separator);

            // Scrollable content area for devices
            scroll = new Gtk.ScrolledWindow ();
            scroll.set_policy (Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC);
            scroll.vexpand = false;
            scroll.set_min_content_height (240);

            content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.halign = Gtk.Align.FILL;

            scroll.set_child (content_box);
            append (scroll);

            // TODO: Populate with actual KDE Connect devices
            update_devices ();

            collapse_button.clicked.connect (() => {
                // Hide the dropdown by setting visible to false
                this.visible = false;
            });
        }

        private void update_devices () {
            // Clear existing items
            while (content_box.get_first_child () != null) {
                content_box.remove (content_box.get_first_child ());
            }

            if (!kde_switch.active) {
                var label = new Gtk.Label ("KDE Connect is off");
                label.set_css_classes ({"QuickSettings-empty-state"});
                label.margin_top = 16;
                label.margin_bottom = 16;
                content_box.append (label);
                return;
            }

            // TODO: Get actual KDE Connect devices from library
            // For now, show a placeholder
            var placeholder = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            placeholder.halign = Gtk.Align.FILL;
            placeholder.margin_start = 8;
            placeholder.margin_end = 8;
            placeholder.margin_top = 16;
            placeholder.margin_bottom = 16;

            var placeholder_label = new Gtk.Label ("No KDE Connect devices found");
            placeholder_label.set_css_classes ({"QuickSettings-empty-state"});

            placeholder.append (placeholder_label);
            content_box.append (placeholder);
        }

        public void add_device_card (string device_name, string device_id) {
            var card = new Gtk.Box (Gtk.Orientation.VERTICAL, 8);
            card.set_css_classes ({"QuickSettings-kde-device-card"});
            card.halign = Gtk.Align.FILL;
            card.margin_start = 8;
            card.margin_end = 8;
            card.margin_top = 8;
            card.margin_bottom = 8;

            // Device header
            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.halign = Gtk.Align.FILL;

            var icon = new Gtk.Image.from_icon_name ("smartphone-symbolic");
            icon.set_icon_size (Gtk.IconSize.LARGE);

            var name_label = new Gtk.Label (device_name);
            name_label.hexpand = true;
            name_label.halign = Gtk.Align.START;
            name_label.set_css_classes ({"QuickSettings-kde-device-name"});

            header.append (icon);
            header.append (name_label);

            card.append (header);

            // Action buttons
            var actions = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 4);
            actions.halign = Gtk.Align.FILL;
            actions.homogeneous = true;

            var sync_clipboard = new Gtk.Button.with_label ("Sync Clipboard");
            var mute_notif = new Gtk.Button.with_label ("Mute Notifications");
            var send_file = new Gtk.Button.with_label ("Send File");

            sync_clipboard.halign = Gtk.Align.FILL;
            mute_notif.halign = Gtk.Align.FILL;
            send_file.halign = Gtk.Align.FILL;

            actions.append (sync_clipboard);
            actions.append (mute_notif);
            actions.append (send_file);

            card.append (actions);

            // TODO: Media controls if applicable
            // if (has_media_playing) {
            //     var media_controls = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            //     // Add media control buttons
            //     card.append (media_controls);
            // }

            content_box.append (card);
        }
    }
}
