namespace Utils {
    public class SystemControlManager : Object {
        private static SystemControlManager? _instance = null;
        
        public BrightnessControl brightness { get; private set; }
        public VolumeControl volume { get; private set; }

        private SystemControlManager () {
            brightness = new BrightnessControl ();
            volume = new VolumeControl ();
        }

        public static SystemControlManager get_instance () {
            if (_instance == null) {
                _instance = new SystemControlManager ();
            }
            return _instance;
        }
    }
}
