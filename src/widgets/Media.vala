namespace Widgets {
    public class Media : Gtk.Box {
        AstalMpris.Mpris mpris = AstalMpris.get_default ();

        public Media () {
            add_css_class ("Media");
            mpris.notify["players"].connect (sync);
            sync ();
        }

        void sync () {
            clear_children (this);

            if (mpris.players.length () == 0) {
                append (new Gtk.Label ("Nothing Playing"));
                return;
            }

            var player = mpris.players.nth_data (0);

            // var cover = new Gtk.Picture () {
            // valign = Gtk.Align.CENTER,
            // content_fit = Gtk.ContentFit.SCALE_DOWN,
            // height_request = 32,
            // };
            // cover.add_css_class ("Cover");

            // update_cover (cover, player.cover_art);

            // var id = player.notify["cover-art"].connect (() => {
            // update_cover (cover, player.cover_art);
            // });

            // cover.destroy.connect (() => player.disconnect (id));

            var label = new Gtk.Label (null);

            player.bind_property ("metadata", label, "label",
                                  BindingFlags.SYNC_CREATE,
                                  (_, src, ref trgt) => {
                var title = player.title;
                var artist = player.artist;
                trgt.set_string (@"$artist - $title");
                return true;
            });

            // append (cover);
            append (label);
        }

        // void update_cover (Gtk.Picture pic, string? art) {
        // if (art == null || art == "") {
        // pic.set_filename (null);
        // return;
        // }

        // try {
        // pic.set_filename (art);
        // } catch (Error e) {
        // warning ("Failed to load cover art: %s", e.message);
        // pic.set_filename (null);
        // }
        // }
    }
}