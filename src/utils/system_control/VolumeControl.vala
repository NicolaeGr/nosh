namespace Utils {
    public class VolumeControl : Object {
        private AstalWp.Audio audio;

        public signal void volume_changed (double volume, double old_volume);

        public const double MAX_VOLUME = 1.5; // 150%
        public const double VOLUME_STEP = 0.05; // 5%

        public VolumeControl () {
            audio = AstalWp.get_default ().audio;

            if (audio.default_speaker != null) {
                audio.default_speaker.notify["volume"].connect (() => {
                    if (audio.default_speaker.volume > MAX_VOLUME) {
                        audio.default_speaker.volume = MAX_VOLUME;
                    }
                });
            }
        }

        public void increase (double step = VOLUME_STEP) {
            var speaker = audio.default_speaker;
            if (speaker == null)return;

            var old_volume = speaker.volume;
            var new_volume = (old_volume + step).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        public void decrease (double step = VOLUME_STEP) {
            var speaker = audio.default_speaker;
            if (speaker == null)return;

            var old_volume = speaker.volume;
            var new_volume = (old_volume - step).clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        public new void set (double volume) {
            var speaker = audio.default_speaker;
            if (speaker == null)return;

            var old_volume = speaker.volume;
            var new_volume = volume.clamp (0, MAX_VOLUME);
            speaker.volume = new_volume;
            volume_changed (new_volume, old_volume);
        }

        public new double get () {
            var speaker = audio.default_speaker;
            if (speaker == null)return 0;
            return speaker.volume;
        }

        public double get_percentage () {
            return get () * 100;
        }
    }
}