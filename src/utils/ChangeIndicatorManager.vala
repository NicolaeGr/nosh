namespace Utils {
    public class ChangeIndicatorManager : Object {
        private static ChangeIndicatorManager? _instance = null;
        private AstalWp.Audio audio;
        
        public signal void volume_changed (double volume, double old_volume);
        public signal void brightness_changed (double brightness, double old_brightness);
        
        public const double MAX_VOLUME = 1.5;  // 150%
        public const double VOLUME_STEP = 0.05;  // 5%
        public const double MAX_BRIGHTNESS = 100.0;  // 100%
        private const string BRIGHTNESS_EXPONENT = "2.0";
        private const string BRIGHTNESS_STEP = "5%";

        private ChangeIndicatorManager () {
            audio = AstalWp.get_default ().audio;
            
            if (audio.default_speaker != null) {
                audio.default_speaker.notify["volume"].connect (() => {
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

        public void increase_volume () {
            var speaker = audio.default_speaker;
            if (speaker == null) return;
            
            var old_volume = speaker.volume;
            var new_volume = (old_volume + VOLUME_STEP).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        public void decrease_volume () {
            var speaker = audio.default_speaker;
            if (speaker == null) return;
            
            var old_volume = speaker.volume;
            var new_volume = (old_volume - VOLUME_STEP).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        public void increase_brightness () {
            var old_brightness = get_brightness ();
            
            try {
                string[] argv = {"brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "set", "+" + BRIGHTNESS_STEP};
                string stdout, stderr;
                int exit_code;
                
                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                    null, out stdout, out stderr, out exit_code);
                
                if (exit_code == 0) {
                    var new_brightness = get_brightness ();
                    brightness_changed (new_brightness, old_brightness);
                }
            } catch (Error e) {
                warning ("Error increasing brightness: %s", e.message);
            }
        }

        public void decrease_brightness () {
            var old_brightness = get_brightness ();
            
            try {
                string[] argv = {"brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "set", BRIGHTNESS_STEP + "-"};
                string stdout, stderr;
                int exit_code;

                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                    null, out stdout, out stderr, out exit_code);
                
                if (exit_code == 0) {
                    var new_brightness = get_brightness ();
                    brightness_changed (new_brightness, old_brightness);
                }
            } catch (Error e) {
                warning ("Error decreasing brightness: %s", e.message);
            }
        }

        public double get_volume_percentage () {
            var speaker = audio.default_speaker;
            if (speaker == null) return 0;
            return speaker.volume * 100;
        }

        private double get_brightness () {
            try {
                string stdout, stderr;
                int exit_code;
                string[] argv = {"brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "-m"};
                
                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                    null, out stdout, out stderr, out exit_code);
                
                if (exit_code != 0) {
                    warning ("Failed to get brightness: %s", stderr);
                    return 0.0;
                }

                var parts = stdout.strip ().split (",");
                if (parts.length >= 4) {
                    var percentage_str = parts[3].replace ("%", "");
                    return double.parse (percentage_str);
                }
                
                return 0.0;
            } catch (Error e) {
                warning ("Error getting brightness: %s", e.message);
                return 0.0;
            }
        }
    }
}
