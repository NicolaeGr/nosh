namespace Utils {
    public class KeybindHandler : Object {
        private static KeybindHandler? _instance = null;
        private ChangeIndicatorManager indicator_manager;

        private KeybindHandler () {
            indicator_manager = ChangeIndicatorManager.get_instance ();
        }

        public static KeybindHandler get_instance () {
            if (_instance == null) {
                _instance = new KeybindHandler ();
            }
            return _instance;
        }

        /**
         * Handle a command from the command line
         * Usage: nosh volume-up, nosh volume-down, nosh brightness-up, nosh brightness-down
         */
        public void handle_command (string command) {
            switch (command) {
                case "volume-up":
                    indicator_manager.increase_volume ();
                    break;
                case "volume-down":
                    indicator_manager.decrease_volume ();
                    break;
                case "brightness-up":
                    indicator_manager.increase_brightness ();
                    break;
                case "brightness-down":
                    indicator_manager.decrease_brightness ();
                    break;
                default:
                    warning ("Unknown command: %s", command);
                    break;
            }
        }
    }
}
