using GLib;

namespace KdeConnect {
    [DBus(name = "org.kde.kdeconnect.daemon")]
    public interface Daemon : GLib.Object {
        [DBus(name = "devices")]
        public abstract string[] devices(bool reachable, bool paired) throws GLib.Error;
        public abstract signal void device_added(string id);
        public abstract signal void device_removed(string id);
        public abstract signal void device_visibility_changed(string id, bool visible);
    }

    [DBus(name = "org.kde.kdeconnect.device")]
    public interface Device : GLib.Object {
        [DBus(name = "id")]
        public abstract string id { owned get; }
        [DBus(name = "name")]
        public abstract string name { owned get; }
        [DBus(name = "isReachable")]
        public abstract bool is_reachable { get; }
        [DBus(name = "isTrusted")]
        public abstract bool is_trusted { get; }
        [DBus(name = "charge")]
        public abstract int charge { get; }
        [DBus(name = "isCharging")]
        public abstract bool is_charging { get; }
        [DBus(name = "isPluginEnabled")]
        public abstract bool is_plugin_enabled(string plugin) throws GLib.Error;

        [DBus(name = "setPluginEnabled")]
        public abstract void set_plugin_enabled(string plugin, bool enabled) throws GLib.Error;
    }

    [DBus(name = "org.kde.kdeconnect.device.clipboard")]
    public interface Clipboard : GLib.Object {
        [DBus(name = "sendClipboard")]
        public abstract void send_clipboard() throws GLib.Error;
    }

    [DBus(name = "org.kde.kdeconnect.device.share")]
    public interface Share : GLib.Object {
        [DBus(name = "shareUrls")]
        public abstract void share_urls(string[] urls) throws GLib.Error;
    }

    public class Manager : Object {
        private Daemon daemon;
        private const string SERVICE = "org.kde.kdeconnect";
        private const string DAEMON_PATH = "/modules/kdeconnect";
        private uint monitor_timeout = 0;

        public signal void daemon_state_changed(bool running);
        public signal void device_added(string device_id);
        public signal void device_removed(string device_id);
        public signal void device_changed(string device_id);

        public Manager() {
            init_daemon.begin();
            start_daemon_monitor();
        }

        public async void init_daemon() {
            try {
                daemon = yield Bus.get_proxy(BusType.SESSION, SERVICE, DAEMON_PATH);

                daemon_state_changed(true);

                daemon.device_added.connect((id) => {
                    device_added(id);
                });

                daemon.device_removed.connect((id) => {
                    device_removed(id);
                });

                daemon.device_visibility_changed.connect((id, visible) => {
                    device_changed(id);
                });
            } catch (Error e) {
                warning("✗ KDE Connect daemon not available: %s", e.message);
                daemon_state_changed(false);
            }
        }

        public bool is_running() {
            return daemon != null;
        }

        public async bool set_daemon_state(bool enabled) {
            if (enabled) {
                warning("Starting KDE Connect daemon...");
                try {
                    Process.spawn_command_line_async("kdeconnectd");
                } catch (Error e) {
                    warning("  kdeconnectd not found, trying alternative: %s", e.message);
                    try {
                        Process.spawn_command_line_async("/usr/bin/kdeconnectd");
                    } catch (Error e2) {
                        warning("  Alternative also failed: %s", e2.message);
                    }
                }
                GLib.Timeout.add(500, () => {
                    init_daemon.begin();
                    return false;
                });
            } else {
                warning("Stopping KDE Connect daemon...");
                try {
                    Process.spawn_command_line_async("pkill kdeconnectd");
                } catch (Error e) {
                    warning("  pkill failed: %s", e.message);
                    try {
                        Process.spawn_command_line_async("killall kdeconnectd");
                    } catch (Error e2) {
                        warning("  killall also failed: %s", e2.message);
                    }
                }
                daemon_state_changed(false);
            }
            return true;
        }

        public async List<Device> get_active_devices() {
            var devices = new List<Device> ();
            if (daemon == null)return devices;

            try {
                var ids = daemon.devices(true, true);
                foreach (var id in ids) {
                    try {
                        var dev = yield Bus.get_proxy<Device> (BusType.SESSION, SERVICE,
                            DAEMON_PATH + "/devices/" + id);

                        devices.append(dev);
                    } catch (Error e) {
                        warning("    Failed to get device %s: %s", id, e.message);
                    }
                }
            } catch (Error e) {
                warning("  Failed to get devices: %s", e.message);
            }
            return devices;
        }

        public async bool toggle_notifications(string device_id, bool enabled) {
            try {
                var dev = yield Bus.get_proxy<Device> (BusType.SESSION, SERVICE,
                    DAEMON_PATH + "/devices/" + device_id);

                dev.set_plugin_enabled("notifications", enabled);
                dev.set_plugin_enabled("sendnotifications", enabled);
                return true;
            } catch (Error e) {
                warning("  Failed to toggle notifications: %s", e.message);
                return false;
            }
        }

        public async bool get_notifications_enabled(string device_id) {
            try {
                var dev = yield Bus.get_proxy<Device> (BusType.SESSION, SERVICE,
                    DAEMON_PATH + "/devices/" + device_id);

                bool enabled = dev.is_plugin_enabled("notifications");
                return enabled;
            } catch (Error e) {
                warning("  Failed to get notifications status: %s", e.message);
                return false;
            }
        }

        public async bool send_files(string device_id, string[] file_paths) {
            try {
                var share = yield Bus.get_proxy<Share> (BusType.SESSION, SERVICE,
                    DAEMON_PATH + "/devices/" + device_id + "/share");

                string[] uris = {};
                foreach (var path in file_paths) {
                    var file = File.new_for_path(path);
                    uris += file.get_uri();
                }

                share.share_urls(uris);
                return true;
            } catch (Error e) {
                warning("  Failed to send files: %s", e.message);
                return false;
            }
        }

        private void start_daemon_monitor() {
            monitor_timeout = GLib.Timeout.add_seconds(2, () => {
                check_daemon_availability.begin();
                return true;
            });
        }

        private async void check_daemon_availability() {
            bool is_available = false;

            try {
                yield Bus.get_proxy<Daemon> (BusType.SESSION, SERVICE, DAEMON_PATH);

                is_available = true;
            } catch (Error e) {
                is_available = false;
            }

            if (is_available && daemon == null) {
                yield init_daemon();
            } else if (!is_available && daemon != null) {
                daemon = null;
                daemon_state_changed(false);
            }
        }

        ~Manager() {
            if (monitor_timeout > 0) {
                GLib.Source.remove(monitor_timeout);
            }
        }
    }
}