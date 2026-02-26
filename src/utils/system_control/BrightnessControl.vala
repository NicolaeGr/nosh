namespace Utils {
    public class BrightnessControl : Object {
        public signal void brightness_changed (double brightness, double old_brightness);

        public const double MAX_BRIGHTNESS = 100.0; // 100%
        private const string BRIGHTNESS_EXPONENT = "2.0";
        public const double DEFAULT_STEP = 5.0; // 5%

        public BrightnessControl () {
        }

        public void increase (double step = DEFAULT_STEP) {
            var old_brightness = get ();

            try {
                string[] argv = { "brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "set", @"+$(step)%" };
                string stdout, stderr;
                int exit_code;

                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                                    null, out stdout, out stderr, out exit_code);

                if (exit_code == 0) {
                    var new_brightness = get ();
                    brightness_changed (new_brightness, old_brightness);
                }
            } catch (Error e) {
                warning ("Error increasing brightness: %s", e.message);
            }
        }

        public void decrease (double step = DEFAULT_STEP) {
            var old_brightness = get ();

            try {
                string[] argv = { "brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "set", @"$(step)%-" };
                string stdout, stderr;
                int exit_code;

                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                                    null, out stdout, out stderr, out exit_code);

                if (exit_code == 0) {
                    var new_brightness = get ();
                    brightness_changed (new_brightness, old_brightness);
                }
            } catch (Error e) {
                warning ("Error decreasing brightness: %s", e.message);
            }
        }

        public new void set (double brightness) {
            var old_brightness = get ();
            var clamped = brightness.clamp (0, MAX_BRIGHTNESS);

            try {
                string[] argv = { "brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "set", @"$(clamped)%" };
                string stdout, stderr;
                int exit_code;

                Process.spawn_sync (null, argv, null, SpawnFlags.SEARCH_PATH,
                                    null, out stdout, out stderr, out exit_code);

                if (exit_code == 0) {
                    var new_brightness = get ();
                    brightness_changed (new_brightness, old_brightness);
                }
            } catch (Error e) {
                warning ("Error setting brightness: %s", e.message);
            }
        }

        public new double get () {
            try {
                string stdout, stderr;
                int exit_code;
                string[] argv = { "brightnessctl", "--exponent=" + BRIGHTNESS_EXPONENT, "-m" };

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

        public double get_percentage () {
            return get ();
        }
    }
}