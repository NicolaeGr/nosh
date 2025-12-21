namespace Widgets {
    public class Time : Gtk.Box {
        private Gtk.Label label;
        private string format;
        private uint interval;

        public Time (string format = "%H:%M - %a %e") {
            this.format = format;
            set_css_classes ({ "Time" });

            label = new Gtk.Label (new DateTime.now_local ().format (format)) { visible = true };
            append (label);

            interval = Timeout.add (1000, () => {
                label.label = new DateTime.now_local ().format (format);
                return Source.CONTINUE;
            }, Priority.DEFAULT);

            destroy.connect (() => Source.remove (interval));
        }
    }
}