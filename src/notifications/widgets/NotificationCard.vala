using Gtk;

namespace Notifications.Widgets {
    public class NotificationCard : Gtk.Box {
        public string title { get; set; }
        public string description { get; set; }
        public string icon_name { get; set; }
        public Gtk.Box actions_box { get; private set; }
        private Gtk.Revealer content_revealer;
        private bool is_expanded = true;

        private uint dismiss_timeout = 0;
        private uint timeout_ms_original = 0;
        private uint64 pause_start_time = 0;
        private AstalNotifd.Notification? notification = null;

        public signal void dismissed ();
        public signal void hover_enter ();
        public signal void hover_leave ();

        public NotificationCard (
            string title,
            string description,
            string icon_name,
            AstalNotifd.Notification? notification = null,
            uint timeout_ms = 5000
        ) {
            Object (
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 4
            );

            this.notification = notification;

            this.title = title;
            this.description = description;
            this.icon_name = icon_name;
            this.timeout_ms_original = timeout_ms;

            set_css_classes ({"NotificationCard", "p-2", "gap-1", "rounded-2xl"});
            set_margin_top (8);
            //  set_margin_bottom (8);
            set_margin_start (8);
            set_margin_end (8);
            set_size_request (360, -1);

            var header = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            header.set_css_classes ({"gap-2", "header"});

            Gtk.Image app_icon;
            if (notification != null && notification.image != null && notification.image.length > 0) {
                var image_path = notification.image;
                if (GLib.FileUtils.test (image_path, GLib.FileTest.EXISTS)) {
                    app_icon = new Gtk.Image.from_file (image_path);
                    app_icon.set_pixel_size (32);
                } else {
                    var resolved_icon = LookupIcon.lookup (icon_name, "application-x-executable");
                    app_icon = new Gtk.Image.from_icon_name (resolved_icon);
                    app_icon.set_icon_size (Gtk.IconSize.NORMAL);
                }
            } else {
                var resolved_icon = LookupIcon.lookup (icon_name, "application-x-executable");
                app_icon = new Gtk.Image.from_icon_name (resolved_icon);
                app_icon.set_icon_size (Gtk.IconSize.NORMAL);
            }

            var title_label = new Gtk.Label (title);
            title_label.set_css_classes ({"heading"});
            title_label.set_halign (Align.START);
            title_label.set_hexpand (true);
            title_label.set_ellipsize (Pango.EllipsizeMode.END);
            title_label.set_lines (1);

            var now = new GLib.DateTime.now_local ();
            var time_label = new Gtk.Label (now.format ("%H:%M"));
            time_label.set_css_classes ({"heading"});
            time_label.set_halign (Align.END);

            var buttons_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 4);
            buttons_box.set_css_classes ({"gap-1"});

            var expand_button = new Gtk.Button ();
            expand_button.set_icon_name ("pan-down-symbolic");
            expand_button.set_css_classes ({"min-w-0",  "min-h-0","square-icon" });
            expand_button.clicked.connect (() => {
                is_expanded = !is_expanded;
                content_revealer.set_reveal_child (is_expanded);
                expand_button.set_icon_name (is_expanded ? "pan-down-symbolic" : "pan-up-symbolic");
            });

            var close_button = new Gtk.Button ();
            close_button.set_icon_name ("window-close-symbolic");
            close_button.set_css_classes ({"min-w-0",  "min-h-0", "square-icon" });
            close_button.clicked.connect (() => {
                this.dismiss ();
            });

            buttons_box.append (expand_button);
            buttons_box.append (close_button);

            header.append (app_icon);
            header.append (title_label);
            header.append (time_label);
            header.append (buttons_box);

            var content_box = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            content_box.set_spacing (8);

            var desc_label = new Gtk.Label (description);
            desc_label.set_css_classes ({"dimmed"});
            desc_label.set_halign (Align.START);
            desc_label.set_wrap (true);
            desc_label.set_lines (2);
            desc_label.set_ellipsize (Pango.EllipsizeMode.END);

            var main_content = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            main_content.set_css_classes ({"gap-2"});
            main_content.append (desc_label);

            actions_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 4);
            actions_box.set_css_classes ({"gap-1", "mt-2"});
            actions_box.set_halign (Align.FILL);
            actions_box.set_homogeneous (true);

            content_box.append (main_content);
            content_box.append (actions_box);

            content_revealer = new Gtk.Revealer ();
            content_revealer.set_reveal_child (is_expanded);
            content_revealer.set_transition_duration (200);
            content_revealer.set_transition_type (Gtk.RevealerTransitionType.SLIDE_DOWN);
            content_revealer.set_child (content_box);

            append (header);
            append (content_revealer);

            add_css_class ("notification-card-appear");

            var motion = new Gtk.EventControllerMotion ();
            motion.enter.connect (() => {
                pause_timer ();
                hover_enter ();
            });
            motion.leave.connect (() => {
                resume_timer ();
                hover_leave ();
            });
            add_controller (motion);

            if (timeout_ms > 0) {
                set_dismiss_timer (timeout_ms);
            }
        }

        private void set_dismiss_timer (uint timeout_ms) {
            dismiss_timeout = Timeout.add (timeout_ms, () => {
                this.dismiss ();
                return Source.REMOVE;
            });
        }

        public void pause_timer () {
            if (dismiss_timeout != 0) {
                GLib.Source.remove (dismiss_timeout);
                dismiss_timeout = 0;
                pause_start_time = GLib.get_monotonic_time ();
            }
        }

        public void resume_timer () {
            if (pause_start_time > 0 && timeout_ms_original > 0) {
                uint64 elapsed = (GLib.get_monotonic_time () - pause_start_time) / 1000;
                uint remaining = (elapsed < timeout_ms_original) ? (uint)(timeout_ms_original - elapsed) : 0;
                pause_start_time = 0;
                set_dismiss_timer (remaining);
            }
        }

        public Gtk.Button add_action (string label) {
            var button = new Gtk.Button.with_label (label);
            button.set_css_classes ({"text-button", "rounded-md"});
            actions_box.append (button);
            return button;
        }

        public void dismiss () {
            if (dismiss_timeout != 0) {
                GLib.Source.remove (dismiss_timeout);
                dismiss_timeout = 0;
            }

            if (notification != null) {
                notification.dismiss ();
            }

            add_css_class ("notification-card-dismiss");
            
            var timeout = Timeout.add (250, () => {
                dismissed ();
                return Source.REMOVE;
            });
        }
    }
}
