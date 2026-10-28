//! The in-memory notification store.
//!
//! IDs start at 1. A replacement reuses the existing ID and keeps its position
//! in the list.

use std::collections::BTreeMap;

use crate::notifications::model::Notification;

#[derive(Debug, Clone)]
pub struct NotificationStore {
    next_id: u32,
    entries: BTreeMap<u32, Notification>,
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationStore {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    pub fn set_next_id(&mut self, next_id: u32) {
        self.next_id = next_id.max(1);
    }

    pub fn allocate_id(&mut self) -> u32 {
        let mut id = self.next_id.max(1);
        while self.entries.contains_key(&id) {
            id = id.wrapping_add(1).max(1);
        }
        self.next_id = id.wrapping_add(1).max(1);
        id
    }

    pub fn contains(&self, id: u32) -> bool {
        self.entries.contains_key(&id)
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.entries.get(&id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Notification> {
        self.entries.get_mut(&id)
    }

    /// Inserts the notification and returns whether it replaced an existing one.
    pub fn insert(&mut self, notification: Notification) -> bool {
        let replaced = self.entries.insert(notification.id, notification).is_some();
        if !replaced {
            self.next_id = self.next_id.max(1);
        }
        replaced
    }

    pub fn remove(&mut self, id: u32) -> Option<Notification> {
        self.entries.remove(&id)
    }

    /// The visible order: by ID, which is the arrival order.
    pub fn snapshot(&self) -> Vec<Notification> {
        self.entries.values().cloned().collect()
    }

    /// Replaces the store with restored notifications and moves `next_id` above
    /// the highest restored ID.
    pub fn restore(&mut self, notifications: Vec<Notification>, next_id: u32) {
        self.entries.clear();
        for notification in notifications {
            self.entries.insert(notification.id, notification);
        }
        let highest = self.entries.keys().next_back().copied().unwrap_or(0);
        self.next_id = next_id.max(highest.saturating_add(1)).max(1);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.entries.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::model::NotificationState;

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
    fn ids_start_at_one_and_increase() {
        let mut store = NotificationStore::new();
        let first = store.allocate_id();
        store.insert(notification(first));
        let second = store.allocate_id();

        assert_eq!((first, second), (1, 2));
        assert_eq!(store.next_id(), 3);
    }

    #[test]
    fn allocation_skips_existing_ids() {
        let mut store = NotificationStore::new();
        store.insert(notification(1));
        store.insert(notification(2));
        store.set_next_id(2);

        assert_eq!(store.allocate_id(), 3);
    }

    #[test]
    fn replacing_keeps_the_id_and_position() {
        let mut store = NotificationStore::new();
        store.insert(notification(1));
        store.insert(notification(2));

        let mut replacement = notification(1);
        replacement.summary = "new".to_string();
        assert!(store.insert(replacement));

        let snapshot = store.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].id, 1);
        assert_eq!(snapshot[0].summary, "new");
    }

    #[test]
    fn restore_moves_the_next_id_above_the_highest_entry() {
        let mut store = NotificationStore::new();
        store.restore(vec![notification(3), notification(7)], 2);

        assert_eq!(store.next_id(), 8);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn removing_returns_the_notification() {
        let mut store = NotificationStore::new();
        store.insert(notification(4));

        assert_eq!(store.remove(4).map(|entry| entry.id), Some(4));
        assert!(store.remove(4).is_none());
    }
}
