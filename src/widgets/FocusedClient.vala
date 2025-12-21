namespace Widgets {
    public class FocusedClient : Gtk.Box {
        public FocusedClient () {
            set_css_classes ({ "Focused" });
            AstalHyprland.get_default ().notify["focused-client"].connect (sync);
            sync ();
        }

        void sync () {
            clear_children (this);

            var client = AstalHyprland.get_default ().focused_client;
            if (client == null)
                return;

            var label = new Gtk.Label (client.title) { visible = true };
            client.bind_property ("title", label, "label", BindingFlags.SYNC_CREATE);
            append (label);
        }
    }
}