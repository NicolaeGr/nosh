using GLib;

namespace KdeConnect {
    [DBus (name = "org.kde.kdeconnect.daemon")]
    public interface Daemon : GLib.Object {
        [DBus (name = "devices")]
        public abstract string[] devices(bool reachable, bool paired) throws GLib.Error;
        public abstract signal void device_added(string id);
        public abstract signal void device_removed(string id);
        public abstract signal void device_visibility_changed(string id, bool visible);
    }
    
    [DBus (name = "org.kde.kdeconnect.device")]
    public interface Device : GLib.Object {
        [DBus (name = "id")]
        public abstract string id { owned get; }
        [DBus (name = "name")]
        public abstract string name { owned get; }
        [DBus (name = "isReachable")]
        public abstract bool is_reachable { get; }
        [DBus (name = "isTrusted")]
        public abstract bool is_trusted { get; }
        [DBus (name = "charge")]
        public abstract int charge { get; }
        [DBus (name = "isCharging")]
        public abstract bool is_charging { get; }
        [DBus (name = "isPluginEnabled")]
        public abstract bool is_plugin_enabled(string plugin) throws GLib.Error;
        [DBus (name = "setPluginEnabled")]
        public abstract void set_plugin_enabled(string plugin, bool enabled) throws GLib.Error;
    }
    
    [DBus (name = "org.kde.kdeconnect.device.clipboard")]
    public interface Clipboard : GLib.Object {
        [DBus (name = "sendClipboard")]
        public abstract void send_clipboard() throws GLib.Error;
    }
    
    [DBus (name = "org.kde.kdeconnect.device.share")]
    public interface Share : GLib.Object {
        [DBus (name = "shareUrls")]
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
                warning("✓ Connected to KDE Connect daemon");
                daemon_state_changed(true);
                
                daemon.device_added.connect((id) => {
                    warning("  → Device added: %s", id);
                    device_added(id);
                });
                
                daemon.device_removed.connect((id) => {
                    warning("  → Device removed: %s", id);
                    device_removed(id);
                });
                
                daemon.device_visibility_changed.connect((id, visible) => {
                    warning("  → Device visibility changed: %s (visible=%s)", id, visible.to_string());
                    device_changed(id);
                });
                
                // Get and log initial device list
                try {
                    string[] ids = daemon.devices(true, true);
                    warning("  Initial device list: %u devices", ids.length);
                    foreach (var id in ids) {
                        warning("    - %s", id);
                    }
                } catch (Error e) {
                    warning("  Failed to get initial device list: %s", e.message);
                }
                
            } catch (Error e) {
                warning("✗ KDE Connect daemon not available: %s", e.message);
                daemon_state_changed(false);
            }
        }
        
        public bool is_running() {
            return daemon != null;
        }
        
        public async bool set_daemon_state(bool enabled) {
            try {
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
                    // Wait a moment for daemon to start
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
            } catch (Error e) {
                warning("Failed to control daemon: %s", e.message);
                return false;
            }
        }
        
        public async List<Device> get_active_devices() {
            var devices = new List<Device>();
            if (daemon == null) return devices;
            
            try {
                var ids = daemon.devices(true, true);
                warning("  Fetching %u devices", ids.length);
                foreach (var id in ids) {
                    try {
                        var dev = yield Bus.get_proxy<Device>(BusType.SESSION, SERVICE, 
                            DAEMON_PATH + "/devices/" + id);
                        warning("    Device: %s = '%s' (reachable=%s, trusted=%s)", 
                            id, dev.name, dev.is_reachable.to_string(), dev.is_trusted.to_string());
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
                var dev = yield Bus.get_proxy<Device>(BusType.SESSION, SERVICE, 
                    DAEMON_PATH + "/devices/" + device_id);
                dev.set_plugin_enabled("notifications", enabled);
                dev.set_plugin_enabled("sendnotifications", enabled);
                warning("  Notifications toggled for %s: %s", device_id, enabled.to_string());
                return true;
            } catch (Error e) {
                warning("  Failed to toggle notifications: %s", e.message);
                return false;
            }
        }
        
        public async bool get_notifications_enabled(string device_id) {
            try {
                var dev = yield Bus.get_proxy<Device>(BusType.SESSION, SERVICE, 
                    DAEMON_PATH + "/devices/" + device_id);
                bool enabled = dev.is_plugin_enabled("notifications");
                warning("  Notifications enabled for %s: %s", device_id, enabled.to_string());
                return enabled;
            } catch (Error e) {
                warning("  Failed to get notifications status: %s", e.message);
                return false;
            }
        }
        
        public async bool send_files(string device_id, string[] file_paths) {
            try {
                var share = yield Bus.get_proxy<Share>(BusType.SESSION, SERVICE, 
                    DAEMON_PATH + "/devices/" + device_id + "/share");
                
                string[] uris = {};
                foreach (var path in file_paths) {
                    var file = File.new_for_path(path);
                    uris += file.get_uri();
                    warning("  File URI: %s", file.get_uri());
                }
                
                share.share_urls(uris);
                warning("  Files sent to %s", device_id);
                return true;
            } catch (Error e) {
                warning("  Failed to send files: %s", e.message);
                return false;
            }
        }
        
        private void start_daemon_monitor() {
            // Check daemon availability every 2 seconds
            monitor_timeout = GLib.Timeout.add_seconds(2, () => {
                check_daemon_availability.begin();
                return true;
            });
        }
        
        private async void check_daemon_availability() {
            bool is_available = false;
            
            try {
                var test_daemon = yield Bus.get_proxy<Daemon>(BusType.SESSION, SERVICE, DAEMON_PATH);
                is_available = true;
            } catch (Error e) {
                is_available = false;
            }
            
            // If state changed, emit signal
            if (is_available && daemon == null) {
                warning("✓ Daemon became available, reconnecting...");
                yield init_daemon();
            } else if (!is_available && daemon != null) {
                warning("✗ Daemon disappeared");
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