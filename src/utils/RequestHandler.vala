namespace Utils {
    public class RequestHandler : Object {
        private static RequestHandler? _instance = null;
        private SystemControlManager control_manager;
        private State.AppState app_state;

        private RequestHandler () {
            control_manager = SystemControlManager.get_instance ();
            app_state = State.AppState.get_instance ();
        }

        public static RequestHandler get_instance () {
            if (_instance == null) {
                _instance = new RequestHandler ();
            }
            return _instance;
        }

        public bool handle_command (string command) {
            switch (command) {
                case "volume-up":
                    control_manager.volume.increase ();
                    return true;
                case "volume-down":
                    control_manager.volume.decrease ();
                    return true;
                case "brightness-up":
                    control_manager.brightness.increase ();
                    return true;
                case "brightness-down":
                    control_manager.brightness.decrease ();
                    return true;
                case "app-launcher":
                    app_state.app_launcher_open = !app_state.app_launcher_open;
                    return true;
                default:
                    warning ("Unknown command: %s", command);
                    return false;
            }
        }
    }
}
