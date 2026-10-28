//! Notification state persistence.
//!
//! Only non-transient notifications are written. The file is replaced
//! atomically: a temporary file in the same directory is renamed over the
//! destination.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use zbus::zvariant::{OwnedValue, Value};

use crate::error::NotificationError;
use crate::notifications::model::{Notification, NotificationAction, NotificationState};
use crate::notifications::store::NotificationStore;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(default)]
    pub next_id: u32,
    #[serde(default)]
    pub notifications: Vec<PersistedNotification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedNotification {
    pub id: u32,
    pub app_name: String,
    #[serde(default)]
    pub app_icon: String,
    pub summary: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub actions: Vec<NotificationAction>,
    #[serde(default)]
    pub hints: HashMap<String, Json>,
    #[serde(default)]
    pub expire_timeout: i32,
    #[serde(default)]
    pub time: i64,
    #[serde(default)]
    pub transient: bool,
}

impl PersistedNotification {
    pub fn into_notification(self) -> Notification {
        Notification {
            id: self.id,
            app_name: self.app_name,
            app_icon: self.app_icon,
            summary: self.summary,
            body: self.body,
            actions: self.actions,
            hints: json_to_hints(self.hints),
            expire_timeout: self.expire_timeout,
            time: self.time,
            state: NotificationState::Received,
        }
    }
}

impl From<&Notification> for PersistedNotification {
    fn from(notification: &Notification) -> Self {
        Self {
            id: notification.id,
            app_name: notification.app_name.clone(),
            app_icon: notification.app_icon.clone(),
            summary: notification.summary.clone(),
            body: notification.body.clone(),
            actions: notification.actions.clone(),
            hints: hints_to_json(&notification.hints),
            expire_timeout: notification.expire_timeout,
            time: notification.time,
            transient: notification.is_transient(),
        }
    }
}

/// Reads the state file. A missing file is an empty state.
pub fn load(path: &Path) -> Result<PersistedState, NotificationError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersistedState::default())
        }
        Err(error) => {
            return Err(NotificationError::Read {
                path: path.display().to_string(),
                message: error.to_string(),
            })
        }
    };

    serde_json::from_slice(&bytes).map_err(|error| NotificationError::Parse {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

/// Writes the non-transient notifications and the next ID atomically.
pub fn save(path: &Path, store: &NotificationStore) -> Result<(), NotificationError> {
    let state = PersistedState {
        next_id: store.next_id(),
        notifications: store
            .iter()
            .filter(|notification| !notification.is_transient())
            .map(PersistedNotification::from)
            .collect(),
    };
    let bytes = serde_json::to_vec(&state).map_err(|error| NotificationError::Parse {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return Err(NotificationError::Write {
                path: path.display().to_string(),
                message: error.to_string(),
            });
        }
    }

    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    if let Err(error) = std::fs::write(&tmp, &bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(NotificationError::Write {
            path: path.display().to_string(),
            message: error.to_string(),
        });
    }
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(NotificationError::Write {
            path: path.display().to_string(),
            message: error.to_string(),
        });
    }

    Ok(())
}

fn hints_to_json(hints: &HashMap<String, OwnedValue>) -> HashMap<String, Json> {
    hints
        .iter()
        .map(|(key, value)| (key.clone(), hint_to_json(value)))
        .collect()
}

fn hint_to_json(value: &OwnedValue) -> Json {
    match &**value {
        Value::Bool(value) => Json::Bool(*value),
        Value::U8(value) => Json::from(*value),
        Value::I16(value) => Json::from(*value),
        Value::U16(value) => Json::from(*value),
        Value::I32(value) => Json::from(*value),
        Value::U32(value) => Json::from(*value),
        Value::I64(value) => Json::from(*value),
        Value::U64(value) => Json::from(*value),
        Value::F64(value) => Json::from(*value),
        Value::Str(value) => Json::String(value.as_str().to_string()),
        Value::Signature(value) => Json::String(value.to_string()),
        Value::ObjectPath(value) => Json::String(value.as_str().to_string()),
        Value::Array(_) => {
            if let Ok(values) = Vec::<String>::try_from(value.clone()) {
                return Json::Array(values.into_iter().map(Json::String).collect());
            }
            if let Ok(values) = Vec::<u8>::try_from(value.clone()) {
                return Json::Array(values.into_iter().map(Json::from).collect());
            }
            if let Ok(values) = Vec::<OwnedValue>::try_from(value.clone()) {
                return Json::Array(values.iter().map(hint_to_json).collect());
            }
            Json::String(format!("{value:?}"))
        }
        Value::Dict(_) => {
            if let Ok(values) = HashMap::<String, OwnedValue>::try_from(value.clone()) {
                let mut object = serde_json::Map::new();
                for (key, entry) in values {
                    object.insert(key, hint_to_json(&entry));
                }
                Json::Object(object)
            } else {
                Json::String(format!("{value:?}"))
            }
        }
        other => Json::String(format!("{other:?}")),
    }
}

fn json_to_hints(hints: HashMap<String, Json>) -> HashMap<String, OwnedValue> {
    hints
        .into_iter()
        .filter_map(|(key, value)| json_to_hint(&value).map(|value| (key, value)))
        .collect()
}

fn json_to_hint(value: &Json) -> Option<OwnedValue> {
    match value {
        Json::Bool(value) => Some(OwnedValue::from(*value)),
        Json::Number(value) => {
            if let Some(value) = value.as_i64() {
                Some(OwnedValue::from(value))
            } else if let Some(value) = value.as_u64() {
                Some(OwnedValue::from(value))
            } else {
                value.as_f64().map(OwnedValue::from)
            }
        }
        Json::String(value) => owned_string(value.clone()),
        Json::Array(values) => {
            if values.iter().all(Json::is_string) {
                let strings: Vec<String> = values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect();
                return OwnedValue::try_from(Value::from(strings)).ok();
            }
            let owned: Vec<OwnedValue> = values.iter().filter_map(json_to_hint).collect();
            OwnedValue::try_from(Value::from(owned)).ok()
        }
        Json::Object(values) => {
            let mut object = HashMap::new();
            for (key, value) in values {
                if let Some(value) = json_to_hint(value) {
                    object.insert(key.clone(), value);
                }
            }
            OwnedValue::try_from(Value::from(object)).ok()
        }
        Json::Null => None,
    }
}

fn owned_string(value: String) -> Option<OwnedValue> {
    OwnedValue::try_from(Value::from(value)).ok()
}

/// Restores a store from persisted state. Transient entries are skipped.
pub fn restore(store: &mut NotificationStore, state: PersistedState) {
    let notifications: Vec<Notification> = state
        .notifications
        .into_iter()
        .filter(|notification| !notification.transient)
        .map(PersistedNotification::into_notification)
        .collect();
    store.restore(notifications, state.next_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::model::HINT_IMAGE_PATH;
    use crate::notifications::model::HINT_URGENCY;

    fn notification(id: u32, transient: bool) -> Notification {
        let mut hints = HashMap::new();
        hints.insert(HINT_URGENCY.to_string(), OwnedValue::from(2u8));
        hints.insert(
            HINT_IMAGE_PATH.to_string(),
            owned_string("/tmp/notification.png".to_string()).unwrap(),
        );
        hints.insert(
            "desktop-entry".to_string(),
            owned_string("app".to_string()).unwrap(),
        );
        hints.insert("unknown-flag".to_string(), OwnedValue::from(true));
        if transient {
            hints.insert("transient".to_string(), OwnedValue::from(true));
        }

        Notification {
            id,
            app_name: "app".to_string(),
            app_icon: "icon".to_string(),
            summary: "summary".to_string(),
            body: "<b>body</b>".to_string(),
            actions: vec![NotificationAction {
                id: "default".to_string(),
                label: "Open".to_string(),
            }],
            hints,
            expire_timeout: 5000,
            time: 1_700_000_000,
            state: NotificationState::Received,
        }
    }

    #[test]
    fn a_missing_state_file_is_empty() {
        let directory = tempfile::tempdir().unwrap();
        let state = load(&directory.path().join("notifications.json")).unwrap();

        assert_eq!(state.next_id, 0);
        assert!(state.notifications.is_empty());
    }

    #[test]
    fn state_round_trips_with_hints() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state").join("notifications.json");
        let mut store = NotificationStore::new();
        store.insert(notification(1, false));
        store.allocate_id();

        save(&path, &store).unwrap();
        let state = load(&path).unwrap();
        let mut restored = NotificationStore::new();
        restore(&mut restored, state);

        assert_eq!(restored.next_id(), 3);
        let entries = restored.snapshot();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "summary");
        assert_eq!(entries[0].body, "<b>body</b>");
        assert_eq!(entries[0].actions[0].label, "Open");
        assert_eq!(
            entries[0].urgency(),
            crate::notifications::model::Urgency::Critical
        );
        assert_eq!(
            entries[0].image_path().as_deref(),
            Some("/tmp/notification.png")
        );
        assert_eq!(entries[0].desktop_entry().as_deref(), Some("app"));
        assert_eq!(
            crate::notifications::model::hint_bool(&entries[0].hints, "unknown-flag"),
            Some(true)
        );
    }

    #[test]
    fn transient_notifications_are_not_written() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notifications.json");
        let mut store = NotificationStore::new();
        store.insert(notification(1, false));
        store.insert(notification(2, true));

        save(&path, &store).unwrap();
        let state = load(&path).unwrap();

        assert_eq!(state.notifications.len(), 1);
        assert_eq!(state.notifications[0].id, 1);
    }

    #[test]
    fn transient_entries_are_skipped_on_restore() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notifications.json");
        let mut persisted = PersistedNotification::from(&notification(3, false));
        persisted.transient = true;
        let state = PersistedState {
            next_id: 4,
            notifications: vec![persisted],
        };
        std::fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();

        let mut store = NotificationStore::new();
        restore(&mut store, load(&path).unwrap());

        assert!(store.is_empty());
        assert_eq!(store.next_id(), 4);
    }

    #[test]
    fn a_corrupt_file_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("notifications.json");
        std::fs::write(&path, b"{not json").unwrap();

        assert!(matches!(load(&path), Err(NotificationError::Parse { .. })));
    }

    #[test]
    fn nested_hints_round_trip() {
        let mut hints = HashMap::new();
        hints.insert(
            "strings".to_string(),
            OwnedValue::try_from(Value::from(vec!["a".to_string(), "b".to_string()])).unwrap(),
        );
        hints.insert(
            "map".to_string(),
            OwnedValue::try_from(Value::from(HashMap::from([(
                "key".to_string(),
                OwnedValue::from(3u8),
            )])))
            .unwrap(),
        );

        let json = hints_to_json(&hints);
        let restored = json_to_hints(json);

        assert_eq!(
            Vec::<String>::try_from(restored.get("strings").unwrap().clone()).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
