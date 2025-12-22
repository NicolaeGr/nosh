namespace TopBar.Widgets {
    public class SysTray : Gtk.Box {
        HashTable<string, Gtk.Widget> items = new HashTable<string, Gtk.Widget> (str_hash, str_equal);
        HashTable<string, ulong> handlers = new HashTable<string, ulong> (str_hash, str_equal);
        AstalTray.Tray tray = AstalTray.get_default ();

        public SysTray () {
            set_css_classes ({ "SysTray" });
            tray.item_added.connect (add_item);
            tray.item_removed.connect (remove_item);
        }

        void add_item (string id) {
            if (items.contains (id))
                return;

            var item = tray.get_item (id);
            if (item == null)
                return;

            // Skip items without icon or menu
            if (item.gicon == null && item.menu_model == null)
                return;

            var btn = new Gtk.MenuButton () { visible = true };
            btn.set_css_classes ({ "menu-icon" });
            btn.set_menu_model (item.menu_model);
            var icon = new Gtk.Image ();

            item.bind_property ("tooltip-markup", btn, "tooltip-markup", BindingFlags.SYNC_CREATE);
            item.bind_property ("gicon", icon, "gicon", BindingFlags.SYNC_CREATE);
            item.bind_property ("menu-model", btn, "menu-model", BindingFlags.SYNC_CREATE);
            btn.insert_action_group ("dbusmenu", item.action_group);
            
            var handler_id = item.notify["action-group"].connect (() => {
                btn.insert_action_group ("dbusmenu", item.action_group);
            });
            handlers.set (id, handler_id);

            btn.set_child (icon);
            append (btn);
            items.set (id, btn);
        }

        void remove_item (string id) {
            var widget = items.get (id);
            if (widget != null) {
                remove (widget);
                items.remove (id);
            }
            
            // Disconnect the handler
            var handler_id = handlers.lookup (id);
            if (handler_id > 0) {
                var item = tray.get_item (id);
                if (item != null) {
                    item.disconnect (handler_id);
                }
                handlers.remove (id);
            }
        }
    }
}
