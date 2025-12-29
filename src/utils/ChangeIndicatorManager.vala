namespace Utils {
    public class ChangeIndicatorManager : Object {
        private static ChangeIndicatorManager? _instance = null;
        private AstalWp.Audio audio;
        
        // Signals
        public signal void volume_changed (double volume, double old_volume);
        public signal void brightness_changed (double brightness, double old_brightness);
        
        // Constants
        public const double MAX_VOLUME = 1.5;  // 150%
        public const double MAX_BRIGHTNESS = 100.0;
        public const double MIN_BRIGHTNESS = 0.0;
        public const double VOLUME_STEP = 0.05;  // 5%
        public const double BRIGHTNESS_STEP = 10.0;  // 10%

        private ChangeIndicatorManager () {
            audio = AstalWp.get_default ().audio;
            
            if (audio.default_speaker != null) {
                audio.default_speaker.notify["volume"].connect (() => {
                    // Ensure volume doesn't exceed our limit
                    if (audio.default_speaker.volume > MAX_VOLUME) {
                        audio.default_speaker.volume = MAX_VOLUME;
                    }
                });
            }
        }

        public static ChangeIndicatorManager get_instance () {
            if (_instance == null) {
                _instance = new ChangeIndicatorManager ();
            }
            return _instance;
        }

        /**
         * Increase volume by one step (5%)
         */
        public void increase_volume () {
            var speaker = audio.default_speaker;
            if (speaker == null) return;
            
            var old_volume = speaker.volume;
            var new_volume = (old_volume + VOLUME_STEP).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        /**
         * Decrease volume by one step (5%)
         */
        public void decrease_volume () {
            var speaker = audio.default_speaker;
            if (speaker == null) return;
            
            var old_volume = speaker.volume;
            var new_volume = (old_volume - VOLUME_STEP).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        /**
         * Increase brightness by one step (10%)
         */
        public void increase_brightness () {
            get_brightness_async.begin ((obj, res) => {
                try {
                    var current = get_brightness_async.end (res);
                    var new_brightness = (current + BRIGHTNESS_STEP).clamp (MIN_BRIGHTNESS, MAX_BRIGHTNESS);
                    set_brightness_internal (new_brightness, current);
                } catch (Error e) {
                    warning ("Error increasing brightness: %s", e.message);
                }
            });
        }

        /**
         * Decrease brightness by one step (10%)
         */
        public void decrease_brightness () {
            get_brightness_async.begin ((obj, res) => {
                try {
                    var current = get_brightness_async.end (res);
                    var new_brightness = (current - BRIGHTNESS_STEP).clamp (MIN_BRIGHTNESS, MAX_BRIGHTNESS);
                    set_brightness_internal (new_brightness, current);
                } catch (Error e) {
                    warning ("Error decreasing brightness: %s", e.message);
                }
            });
        }

        /**
         * Get current volume as percentage (0-150)
         */
        public double get_volume_percentage () {
            var speaker = audio.default_speaker;
            if (speaker == null) return 0;
            return speaker.volume * 100;
        }

        /**
         * Get current brightness as percentage (0-100)
         */
        public void get_brightness_async_wrapper (SourceFunc callback) {
            get_brightness_async.begin ((obj, res) => {
                try {
                    get_brightness_async.end (res);
                    callback ();
                } catch (Error e) {
                    warning ("Error getting brightness: %s", e.message);
                }
            });
        }

        private async double get_brightness_async () throws Error {
            string stdout, stderr;
            int exit_code;
            string[] argv = {"light", "-G"};
            
            yield;  // Yield to avoid blocking
            
            Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                null, out stdout, out stderr, out exit_code);
            
            if (exit_code != 0) {
                throw new IOError.FAILED ("Failed to get brightness");
            }
            
            return double.parse (stdout.strip ());
        }

        private void set_brightness_internal (double new_brightness, double old_brightness) {
            Thread<bool> thread = new Thread<bool> ("brightness-setter", () => {
                try {
                    string[] argv = {"light", "-S", new_brightness.to_string ()};
                    string stdout, stderr;
                    int exit_code;
                    
                    Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                        null, out stdout, out stderr, out exit_code);
                    
                    if (exit_code == 0) {
                        brightness_changed (new_brightness, old_brightness);
                    }
                } catch (Error e) {
                    warning ("Error setting brightness: %s", e.message);
                }
                return true;
            });
            thread.join ();
        }
    }
}
