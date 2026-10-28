//! Expired notifications kept for the centre panel.
//!
//! The daemon drops a notification when it expires. The UI keeps a copy of the
//! ones that ran out on their own, so the panel can list them after the popup
//! is gone. A notification the user dismisses or acts on is forgotten.

use std::cell::RefCell;
use std::collections::HashMap;

use nosh_core::{ClosedReason, Notification};

#[derive(Default)]
pub struct NotificationHistory {
    pending: RefCell<HashMap<u32, Notification>>,
    expired: RefCell<Vec<Notification>>,
}

impl NotificationHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers a notification while it can still expire.
    pub fn remember(&self, notification: &Notification) {
        self.pending
            .borrow_mut()
            .insert(notification.id, notification.clone());
    }

    /// Resolves a notification. Expired ones move into the panel list, newest
    /// first. Any other close reason drops them.
    pub fn close(&self, id: u32, reason: ClosedReason) {
        let Some(notification) = self.pending.borrow_mut().remove(&id) else {
            return;
        };
        if reason != ClosedReason::Expired {
            return;
        }

        let mut expired = self.expired.borrow_mut();
        if expired.iter().all(|entry| entry.id != id) {
            expired.insert(0, notification);
        }
    }

    /// The expired notifications, newest first.
    pub fn entries(&self) -> Vec<Notification> {
        self.expired.borrow().clone()
    }

    /// Drops one expired notification from the panel list.
    pub fn remove(&self, id: u32) {
        self.expired.borrow_mut().retain(|entry| entry.id != id);
    }

    pub fn len(&self) -> usize {
        self.expired.borrow().len()
    }

    pub fn clear(&self) {
        self.expired.borrow_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nosh_core::NotificationState;

    fn notification(id: u32) -> Notification {
        Notification {
            id,
            app_name: "app".to_string(),
            app_icon: String::new(),
            summary: format!("summary {id}"),
            body: String::new(),
            actions: Vec::new(),
            hints: Default::default(),
            expire_timeout: -1,
            time: 0,
            state: NotificationState::Received,
        }
    }

    #[test]
    fn an_expired_notification_moves_into_the_list() {
        let history = NotificationHistory::new();
        history.remember(&notification(1));
        history.close(1, ClosedReason::Expired);

        assert_eq!(history.len(), 1);
        assert_eq!(history.entries()[0].id, 1);
    }

    #[test]
    fn a_dismissed_notification_is_forgotten() {
        let history = NotificationHistory::new();
        history.remember(&notification(1));
        history.close(1, ClosedReason::DismissedByUser);

        assert_eq!(history.len(), 0);
    }

    #[test]
    fn newer_expirations_come_first() {
        let history = NotificationHistory::new();
        for id in [1, 2, 3] {
            history.remember(&notification(id));
            history.close(id, ClosedReason::Expired);
        }

        let ids: Vec<u32> = history.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [3, 2, 1]);
    }

    #[test]
    fn removing_one_entry_keeps_the_rest() {
        let history = NotificationHistory::new();
        for id in [1, 2, 3] {
            history.remember(&notification(id));
            history.close(id, ClosedReason::Expired);
        }
        history.remove(2);

        let ids: Vec<u32> = history.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(ids, [3, 1]);
    }

    #[test]
    fn clearing_leaves_no_entries() {
        let history = NotificationHistory::new();
        history.remember(&notification(1));
        history.close(1, ClosedReason::Expired);
        history.clear();

        assert_eq!(history.len(), 0);
    }
}
