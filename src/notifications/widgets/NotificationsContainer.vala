using Gtk;

namespace Notifications.Widgets {
    public class NotificationsContainer : Gtk.Box {
        private Gtk.Box notifications_stack;
        private Queue<NotificationCard> waiting_queue;
        private const uint DEFAULT_TIMEOUT = 5000;
        private const uint MAX_VISIBLE = 3;

        public NotificationsContainer () {
            Object (
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 0
            );

            set_halign (Align.END);
            set_valign (Align.START);
            set_css_classes ({"NotificationsContainer"});
            set_size_request (-1, 0);

            waiting_queue = new Queue<NotificationCard> ();
            notifications_stack = new Gtk.Box (Gtk.Orientation.VERTICAL, 4);
            notifications_stack.set_css_classes ({"notifications-stack"});

            append (notifications_stack);

            try {
                var notifd = AstalNotifd.Notifd.get_default ();
                
                notifd.notified.connect ((notification_id) => {
                    var notification = notifd.get_notification (notification_id);
                    if (notification != null) {
                        on_notification_received (notification);
                    }
                });

                unowned var notifications = notifd.notifications;
                if (notifications != null) {
                    foreach (var notification in notifications) {
                        on_notification_received (notification);
                    }
                }
            } catch (Error e) {
                warning ("Failed to initialize Notifd: %s\n", e.message);
            }
        }

        private void on_notification_received (AstalNotifd.Notification notification) {
            var card = new NotificationCard (
                notification.summary,
                notification.body,
                notification.app_icon,
                notification,
                DEFAULT_TIMEOUT
            );

            unowned var actions = notification.actions;
            if (actions != null) {
                foreach (var action in actions) {
                    var btn = card.add_action (action.label);
                    btn.clicked.connect (() => {
                        notification.invoke (action.id);
                    });
                }
            }

            card.dismissed.connect (() => {
                on_card_dismissed (card);
            });

            add_notification (card);
        }
        public void add_notification (NotificationCard card) {
            var children_count = count_visible_notifications ();
            
            if (children_count < MAX_VISIBLE) {
                notifications_stack.append (card);
                update_pointer_events ();
            } else {
                waiting_queue.push_tail (card);
            }
        }
        
        private uint count_visible_notifications () {
            var count = 0;
            var child = notifications_stack.get_first_child ();
            while (child != null) {
                count++;
                child = child.get_next_sibling ();
            }
            return count;
        }
        
        private void show_next_queued_notification () {
            if (!waiting_queue.is_empty ()) {
                var card = waiting_queue.pop_head ();
                notifications_stack.append (card);
                update_pointer_events ();
            }
        }
        
        private void on_card_dismissed (NotificationCard card) {
            // Check if card is still in the stack before removing
            var child = notifications_stack.get_first_child ();
            var found = false;
            while (child != null) {
                if (child == card) {
                    found = true;
                    break;
                }
                child = child.get_next_sibling ();
            }
            
            if (found) {
                notifications_stack.remove (card);
                show_next_queued_notification ();
            }
            
            on_notification_dismissed ();
        }

        private void on_notification_dismissed () {
            var child = notifications_stack.get_first_child ();
            var has_children = child != null;
            
            var toplevel = (Astal.Window)get_root ();
            if (toplevel != null) {
                toplevel.queue_allocate ();
                
                if (!has_children) {
                    toplevel.visible = false;
                    toplevel.set_size_request (-1, 0);
                }
            }
            
            update_pointer_events ();
        }

        private void update_pointer_events () {
            var child = notifications_stack.get_first_child ();
            if (child == null) {
                add_css_class ("pointer-events-none");
            } else {
                remove_css_class ("pointer-events-none");
                var toplevel = (Astal.Window)get_root ();
                if (toplevel != null) {
                    toplevel.visible = true;
                }
            }
        }
    }
}
