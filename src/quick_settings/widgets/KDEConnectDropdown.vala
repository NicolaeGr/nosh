namespace QuickSettings.Widgets {
    public class KDEConnectDropdown : Gtk.Box {
        private KdeConnect.Manager kde_manager;
        private Gtk.Box devices_box;
        private Gtk.Label status_label;
        private Gtk.Switch daemon_toggle;

        public KDEConnectDropdown() {
            Object(
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 0
            );
        }

        construct {
            add_css_class("dropdown");
            add_css_class("kde-connect-dropdown");
            set_size_request(300, -1);
            
            // Refresh on visibility
            notify["visible"].connect(() => {
                if (get_visible()) {
                    load_and_display_devices.begin();
                }
            });

            // Initialize KDE Manager
            kde_manager = new KdeConnect.Manager();
            kde_manager.daemon_state_changed.connect((running) => {
                update_daemon_status(running);
                daemon_toggle.set_active(running);
                if (!running) {
                    clear_devices();
                }
            });

            // Header
            var header = new Gtk.CenterBox();
            header.add_css_class("dropdown-header");

            var title = new Gtk.Label("KDE Connect");
            title.set_markup("<b>KDE Connect</b>");
            header.set_start_widget(title);

            var header_buttons = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 6);
            header_buttons.add_css_class("header-buttons");
            header_buttons.margin_start = 6;

            daemon_toggle = new Gtk.Switch();
            daemon_toggle.set_css_classes ({"small-switch"});
            daemon_toggle.active = false;
            daemon_toggle.halign = Gtk.Align.START;
            daemon_toggle.valign = Gtk.Align.CENTER;
            daemon_toggle.hexpand = true;
            daemon_toggle.notify["active"].connect((pspec) => {
                toggle_daemon.begin(daemon_toggle.active);
            });

            // Refresh button
            var refresh_btn = new Gtk.Button.from_icon_name("view-refresh-symbolic");
            refresh_btn.set_tooltip_text("Refresh devices");
            refresh_btn.clicked.connect(() => {
                warning("KDE: Refreshing devices");
                load_and_display_devices.begin();
            });

            // Settings button
            var settings_btn = new Gtk.Button.from_icon_name("emblem-system-symbolic");
            settings_btn.set_tooltip_text("Open KDE Connect");
            settings_btn.clicked.connect(() => {
                try {
                    AppInfo.launch_default_for_uri("kcmshell5", null);
                } catch (Error e) {
                    warning(@"KDE: Failed to open KDE Connect settings: $(e.message)");
                    try {
                        Process.spawn_command_line_async("kdeconnect-app");
                    } catch (Error e2) {
                        warning(@"KDE: Failed to launch KDE Connect settings: $(e2.message)");
                    }
                }
            });

            header_buttons.append(daemon_toggle);
            header_buttons.append(refresh_btn);
            header_buttons.append(settings_btn);
            header.set_end_widget(header_buttons);

            append(header);

            // Separator
            var separator = new Gtk.Separator(Gtk.Orientation.HORIZONTAL);
            separator.add_css_class("dropdown-separator");
            append(separator);

            // Daemon status and toggle
            //  var daemon_box = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 8);
            //  daemon_box.add_css_class("daemon-box");
            
            //  status_label = new Gtk.Label("");
            //  status_label.add_css_class("daemon-status");
            //  status_label.set_wrap(true);
            //  status_label.set_hexpand(true);
            
           
            
            //  daemon_box.append(status_label);
            //  daemon_box.append(daemon_toggle);
            //  daemon_box.set_margin_start(12);
            //  daemon_box.set_margin_end(12);
            //  daemon_box.set_margin_top(8);
            //  daemon_box.set_margin_bottom(8);
            //  append(daemon_box);

            var separator2 = new Gtk.Separator(Gtk.Orientation.HORIZONTAL);
            separator2.add_css_class("dropdown-separator");
            append(separator2);

            devices_box = new Gtk.Box(Gtk.Orientation.VERTICAL, 4);
            devices_box.add_css_class("devices-container");
            devices_box.set_margin_start(8);
            devices_box.set_margin_end(8);
            devices_box.set_margin_top(4);
            devices_box.set_margin_bottom(4);
            append(devices_box);

            load_and_display_devices.begin();
        }

        private void update_daemon_status(bool running) {
            // Status label is commented out, so we skip updating it
            // Just ensure toggle state is correct
        }

        private async void load_and_display_devices() {
            try {
                // Check if daemon is running first
                if (!kde_manager.is_running()) {
                    warning("KDE: Daemon not running, attempting to reconnect...");
                    yield kde_manager.init_daemon();
                }
                
                var devices = yield kde_manager.get_active_devices();
                
                while (devices_box.get_first_child() != null) {
                    devices_box.remove(devices_box.get_first_child());
                }

                if (devices.length() == 0) {
                    var no_devices = new Gtk.Label("No devices found");
                    no_devices.add_css_class("no-devices");
                    devices_box.append(no_devices);
                    return;
                }

                foreach (var device in devices) {
                    var card = create_device_card(device);
                    devices_box.append(card);
                }
            } catch (Error e) {
                warning(@"KDE: Error loading devices: $(e.message)");
            }
        }

        private void clear_devices() {
            while (devices_box.get_first_child() != null) {
                devices_box.remove(devices_box.get_first_child());
            }
            var no_devices = new Gtk.Label("Daemon offline");
            no_devices.add_css_class("no-devices");
            devices_box.append(no_devices);
        }

        private Gtk.Widget create_device_card(KdeConnect.Device device) {
            string device_id = device.id;
            
            var card = new Gtk.Box(Gtk.Orientation.VERTICAL, 0);
            card.add_css_class("device-card");

            var top_grid = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 8);
            top_grid.add_css_class("device-top");
            
            var device_icon = new Gtk.Image.from_icon_name("phone-symbolic");
            device_icon.set_icon_size(Gtk.IconSize.LARGE);
            device_icon.add_css_class("device-icon");
            
            var title_section = new Gtk.Box(Gtk.Orientation.VERTICAL, 2);
            title_section.set_hexpand(true);
            
            var device_name = new Gtk.Label(device.name);
            device_name.add_css_class("device-name");
            device_name.set_ellipsize(Pango.EllipsizeMode.END);
            device_name.set_halign(Gtk.Align.START);
            
            var status_row = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 12);
            status_row.set_homogeneous(false);
            
            var reachable_status = new Gtk.Label(device.is_reachable ? "Reachable" : "Offline");
            reachable_status.add_css_class("status-label");
            reachable_status.add_css_class(device.is_reachable ? "status-online" : "status-offline");
            reachable_status.set_halign(Gtk.Align.START);
            
            var trust_status = new Gtk.Label(device.is_trusted ? "Trusted" : "Untrusted");
            trust_status.add_css_class("status-label");
            trust_status.add_css_class(device.is_trusted ? "status-trusted" : "status-untrusted");
            trust_status.set_halign(Gtk.Align.START);
            
            status_row.append(reachable_status);
            status_row.append(trust_status);
            
            title_section.append(device_name);
            title_section.append(status_row);
            
            var battery_label = new Gtk.Label(@"$(device.charge)%");
            battery_label.add_css_class("battery-level");
            
            top_grid.append(device_icon);
            top_grid.append(title_section);
            top_grid.append(battery_label);
            top_grid.set_margin_start(8);
            top_grid.set_margin_end(8);
            top_grid.set_margin_top(6);
            top_grid.set_margin_bottom(6);
            
            card.append(top_grid);

            var buttons_box = new Gtk.Box(Gtk.Orientation.HORIZONTAL, 2);
            buttons_box.add_css_class("device-buttons");
            buttons_box.set_homogeneous(true);
            
            var send_clipboard_btn = new Gtk.Button.from_icon_name("edit-copy-symbolic");
            send_clipboard_btn.set_tooltip_text("Send clipboard");
            send_clipboard_btn.add_css_class("device-action-btn");
            send_clipboard_btn.clicked.connect(() => {
                send_clipboard_to_device.begin(device_id);
            });

            var copy_clipboard_btn = new Gtk.Button.from_icon_name("edit-paste-symbolic");
            copy_clipboard_btn.set_tooltip_text("Paste clipboard");
            copy_clipboard_btn.add_css_class("device-action-btn");
            copy_clipboard_btn.clicked.connect(() => {
                copy_clipboard_from_device.begin(device_id);
            });

            var send_file_btn = new Gtk.Button.from_icon_name("document-send-symbolic");
            send_file_btn.set_tooltip_text("Send file");
            send_file_btn.add_css_class("device-action-btn");
            send_file_btn.clicked.connect(() => {
                send_file_to_device.begin(device_id);
            });

            buttons_box.append(send_clipboard_btn);
            buttons_box.append(copy_clipboard_btn);
            buttons_box.append(send_file_btn);
            buttons_box.set_margin_start(8);
            buttons_box.set_margin_end(8);
            buttons_box.set_margin_top(0);
            buttons_box.set_margin_bottom(6);
            
            card.append(buttons_box);

            return card;
        }

        private async void send_clipboard_to_device(string device_id) {
            try {
                warning(@"KDE: Sending clipboard to device $device_id");
                // TODO: Implement clipboard send
            } catch (Error e) {
                warning(@"KDE: Error sending clipboard: $(e.message)");
            }
        }

        private async void copy_clipboard_from_device(string device_id) {
            try {
                warning(@"KDE: Copying clipboard from device $device_id");
                // TODO: Implement clipboard copy
            } catch (Error e) {
                warning(@"KDE: Error copying clipboard: $(e.message)");
            }
        }

        private async void send_file_to_device(string device_id) {
            try {
                warning(@"KDE: Sending file to device $device_id");
                // TODO: Implement file send dialog
            } catch (Error e) {
                warning(@"KDE: Error sending file: $(e.message)");
            }
        }

        private async void toggle_daemon(bool enabled) {
            try {
                warning(@"KDE: Setting daemon to $enabled");
                yield kde_manager.set_daemon_state(enabled);
            } catch (Error e) {
                warning(@"KDE: Error toggling daemon: $(e.message)");
                // Reset toggle on error
                daemon_toggle.set_active(!enabled);
            }
        }
    }
}

