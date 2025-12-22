namespace State {
    public class AppState : Object {
        private static AppState? _instance = null;

        public bool quick_settings_open { get; set; default = false; }
        public string zwp_status { get; set; default = ""; }
        public bool idle_inhibitor_active { get; set; default = false; }

        private Utils.IdleInhibitorManager idle_inhibitor_manager;

        private AppState () {
            idle_inhibitor_manager = Utils.IdleInhibitorManager.get_instance ();
            
            // Bind our property to the manager's state
            idle_inhibitor_manager.bind_property ("active", this, "idle-inhibitor-active", BindingFlags.SYNC_CREATE | BindingFlags.BIDIRECTIONAL);
        }

        public static AppState get_instance () {
            if (_instance == null) {
                _instance = new AppState ();
            }
            return _instance;
        }

        public bool is_idle_inhibitor_available () {
            return idle_inhibitor_manager.is_available ();
        }
    }
}
