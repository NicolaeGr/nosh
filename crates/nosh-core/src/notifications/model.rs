//! Notification payloads and standard hint helpers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::zvariant::OwnedValue;

use crate::monitors::Corner;
use crate::monitors::MonitorPolicy;

pub const SERVER_NAME: &str = "nosh";
pub const SERVER_VENDOR: &str = "nosh";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SPEC_VERSION: &str = "1.2";

/// The capabilities returned by `GetCapabilities`.
pub const DEFAULT_CAPABILITIES: [&str; 9] = [
    "action-icons",
    "actions",
    "body",
    "body-hyperlinks",
    "body-images",
    "body-markup",
    "icon-static",
    "persistence",
    "sound",
];

pub const HINT_IMAGE_DATA: &str = "image-data";
pub const HINT_IMAGE_DATA_LEGACY: &str = "image_data";
pub const HINT_ICON_DATA: &str = "icon_data";
pub const HINT_IMAGE_PATH: &str = "image-path";
pub const HINT_ACTION_ICONS: &str = "action-icons";
pub const HINT_CATEGORY: &str = "category";
pub const HINT_DESKTOP_ENTRY: &str = "desktop-entry";
pub const HINT_RESIDENT: &str = "resident";
pub const HINT_SOUND_FILE: &str = "sound-file";
pub const HINT_SOUND_NAME: &str = "sound-name";
pub const HINT_SUPPRESS_SOUND: &str = "suppress-sound";
pub const HINT_TRANSIENT: &str = "transient";
pub const HINT_URGENCY: &str = "urgency";
pub const HINT_X: &str = "x";
pub const HINT_Y: &str = "y";
pub const HINT_BODY_IMAGES: &str = "body-images";
pub const HINT_BODY_HYPERLINKS: &str = "body-hyperlinks";
pub const HINT_ACTIVATION_TOKEN: &str = "activation-token";

/// The action id freedesktop clients use to dismiss a notification.
pub const ACTION_DISMISS: &str = "dismiss";
/// The action id freedesktop clients use to expire a notification.
pub const ACTION_EXPIRE: &str = "expire";

const MAX_LABEL_CHARS: usize = 256;
const MAX_BODY_BYTES: usize = 64 * 1024;

/// One notification held by the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub hints: HashMap<String, OwnedValue>,
    /// Milliseconds. `-1` means the server default.
    pub expire_timeout: i32,
    /// Unix seconds.
    pub time: i64,
    pub state: NotificationState,
}

impl Notification {
    pub fn action(&self, id: &str) -> Option<&NotificationAction> {
        self.actions.iter().find(|action| action.id == id)
    }

    pub fn has_default_action(&self) -> bool {
        self.action("default").is_some()
    }

    pub fn urgency(&self) -> Urgency {
        Urgency::from_hints(&self.hints)
    }

    pub fn is_transient(&self) -> bool {
        hint_bool(&self.hints, HINT_TRANSIENT).unwrap_or(false)
    }

    pub fn is_resident(&self) -> bool {
        hint_bool(&self.hints, HINT_RESIDENT).unwrap_or(false)
    }

    pub fn image_path(&self) -> Option<String> {
        hint_string(&self.hints, HINT_IMAGE_PATH).filter(|path| !path.is_empty())
    }

    pub fn category(&self) -> Option<String> {
        hint_string(&self.hints, HINT_CATEGORY).filter(|value| !value.is_empty())
    }

    pub fn desktop_entry(&self) -> Option<String> {
        hint_string(&self.hints, HINT_DESKTOP_ENTRY).filter(|value| !value.is_empty())
    }

    pub fn action_icons(&self) -> bool {
        hint_bool(&self.hints, HINT_ACTION_ICONS).unwrap_or(false)
    }

    pub fn suppress_sound(&self) -> bool {
        hint_bool(&self.hints, HINT_SUPPRESS_SOUND).unwrap_or(false)
    }

    pub fn activation_token(&self) -> Option<String> {
        hint_string(&self.hints, HINT_ACTIVATION_TOKEN).filter(|value| !value.is_empty())
    }

    pub fn position_hint(&self) -> Option<(i32, i32)> {
        let x = hint_i32(&self.hints, HINT_X)?;
        let y = hint_i32(&self.hints, HINT_Y)?;
        Some((x, y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationState {
    Received,
    Displayed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationAction {
    pub id: String,
    pub label: String,
}

/// The reason reported in `NotificationClosed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ClosedReason {
    Expired = 1,
    DismissedByUser = 2,
    Closed = 3,
    Undefined = 4,
}

impl ClosedReason {
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Expired,
            2 => Self::DismissedByUser,
            3 => Self::Closed,
            _ => Self::Undefined,
        }
    }
}

/// The urgency hint. A missing or invalid hint is normal urgency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::Critical => 2,
        }
    }

    pub fn from_hints(hints: &HashMap<String, OwnedValue>) -> Self {
        match hint_u8(hints, HINT_URGENCY) {
            Some(0) => Self::Low,
            Some(2) => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// The settings the daemon and the UI read.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationSettings {
    pub ignore_timeout: bool,
    pub dont_disturb: bool,
    /// Milliseconds. Negative means no timeout.
    pub default_timeout: i32,
    pub max_visible: usize,
    /// Milliseconds. The UI card lifetime.
    pub display_timeout: i32,
    pub position: Corner,
    pub monitor_policy: MonitorPolicy,
    pub server_capabilities: Vec<String>,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            ignore_timeout: false,
            dont_disturb: false,
            default_timeout: -1,
            max_visible: 3,
            display_timeout: 5000,
            position: Corner::TopRight,
            monitor_policy: MonitorPolicy::Farthest,
            server_capabilities: DEFAULT_CAPABILITIES
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }
}

impl NotificationSettings {
    /// The timeout in milliseconds, where `-1` selects the default and a
    /// negative default means no timeout.
    pub fn timeout_for(&self, expire_timeout: i32) -> i32 {
        if expire_timeout >= 0 {
            expire_timeout
        } else {
            self.default_timeout
        }
    }
}

/// The notification state shipped to the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationModel {
    pub notifications: Vec<Notification>,
    pub settings: NotificationSettings,
}

impl NotificationModel {
    pub fn dont_disturb(&self) -> bool {
        self.settings.dont_disturb
    }

    pub fn upsert(&mut self, notification: Notification) {
        if let Some(existing) = self
            .notifications
            .iter_mut()
            .find(|entry| entry.id == notification.id)
        {
            *existing = notification;
        } else {
            self.notifications.push(notification);
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.notifications.retain(|entry| entry.id != id);
    }
}

/// Builds actions from the flat `id, label, id, label` array. A trailing
/// unmatched id is ignored.
pub fn parse_actions(raw: &[String]) -> Vec<NotificationAction> {
    raw.chunks_exact(2)
        .filter(|pair| !pair[0].is_empty())
        .map(|pair| NotificationAction {
            id: pair[0].clone(),
            label: pair[1].clone(),
        })
        .collect()
}

/// Strips control characters and caps the length of a single-line field.
pub fn sanitize_label(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let trimmed = cleaned.trim();
    trimmed.chars().take(MAX_LABEL_CHARS).collect()
}

/// Removes NULs and most control characters. Newlines and tabs survive because
/// the body is markup.
pub fn sanitize_body(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(MAX_BODY_BYTES));
    for ch in value.chars() {
        if ch == '\0' || (ch.is_control() && ch != '\n' && ch != '\r' && ch != '\t') {
            continue;
        }
        let len = ch.len_utf8();
        if out.len() + len > MAX_BODY_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

pub fn hint_bool(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    hints.get(key).and_then(|value| bool::try_from(value).ok())
}

pub fn hint_u8(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    let value = hints.get(key)?;
    if let Ok(value) = u8::try_from(value) {
        return Some(value);
    }
    if let Ok(value) = i32::try_from(value) {
        return Some(value.clamp(0, 255) as u8);
    }
    if let Ok(value) = u32::try_from(value) {
        return Some(value.min(255) as u8);
    }
    if let Ok(value) = i64::try_from(value) {
        return Some(value.clamp(0, 255) as u8);
    }
    if let Ok(value) = u64::try_from(value) {
        return Some(value.min(255) as u8);
    }
    None
}

pub fn hint_i32(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    let value = hints.get(key)?;
    if let Ok(value) = i32::try_from(value) {
        return Some(value);
    }
    if let Ok(value) = u32::try_from(value) {
        return Some(value.min(i32::MAX as u32) as i32);
    }
    if let Ok(value) = i64::try_from(value) {
        return Some(value.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    }
    if let Ok(value) = u64::try_from(value) {
        return Some(value.min(i32::MAX as u64) as i32);
    }
    None
}

pub fn hint_string(hints: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = hints.get(key)?;
    String::try_from(value.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn value<T: Into<Value<'static>>>(value: T) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    #[test]
    fn actions_are_built_from_pairs() {
        let raw = vec![
            "default".to_string(),
            "Open".to_string(),
            "reply".to_string(),
            "Reply".to_string(),
        ];

        assert_eq!(
            parse_actions(&raw),
            vec![
                NotificationAction {
                    id: "default".to_string(),
                    label: "Open".to_string(),
                },
                NotificationAction {
                    id: "reply".to_string(),
                    label: "Reply".to_string(),
                },
            ]
        );
    }

    #[test]
    fn a_trailing_action_id_is_ignored() {
        let raw = vec![
            "default".to_string(),
            "Open".to_string(),
            "orphan".to_string(),
        ];

        assert_eq!(parse_actions(&raw).len(), 1);
    }

    #[test]
    fn empty_action_ids_are_dropped() {
        let raw = vec![String::new(), "Open".to_string()];
        assert!(parse_actions(&raw).is_empty());
    }

    #[test]
    fn the_timeout_falls_back_only_for_negative_values() {
        let mut settings = NotificationSettings {
            default_timeout: 8000,
            ..NotificationSettings::default()
        };

        assert_eq!(settings.timeout_for(500), 500);
        assert_eq!(settings.timeout_for(0), 0);
        assert_eq!(settings.timeout_for(-1), 8000);

        settings.default_timeout = -1;
        assert_eq!(settings.timeout_for(-1), -1);
    }

    #[test]
    fn closed_reasons_round_trip() {
        for reason in [
            ClosedReason::Expired,
            ClosedReason::DismissedByUser,
            ClosedReason::Closed,
            ClosedReason::Undefined,
        ] {
            assert_eq!(ClosedReason::from_u32(reason.as_u32()), reason);
        }
        assert_eq!(ClosedReason::from_u32(99), ClosedReason::Undefined);
    }

    #[test]
    fn hints_are_read_with_their_standard_types() {
        let mut hints = HashMap::new();
        hints.insert(HINT_URGENCY.to_string(), value(2u8));
        hints.insert(HINT_TRANSIENT.to_string(), value(true));
        hints.insert(HINT_RESIDENT.to_string(), value(false));
        hints.insert(HINT_CATEGORY.to_string(), value("email".to_string()));
        hints.insert(
            HINT_DESKTOP_ENTRY.to_string(),
            value("thunderbird".to_string()),
        );
        hints.insert(HINT_X.to_string(), value(10i32));
        hints.insert(HINT_Y.to_string(), value(20i32));

        let notification = Notification {
            id: 1,
            app_name: "Mail".to_string(),
            app_icon: String::new(),
            summary: "New mail".to_string(),
            body: String::new(),
            actions: Vec::new(),
            hints,
            expire_timeout: -1,
            time: 0,
            state: NotificationState::Received,
        };

        assert_eq!(notification.urgency(), Urgency::Critical);
        assert!(notification.is_transient());
        assert!(!notification.is_resident());
        assert_eq!(notification.category().as_deref(), Some("email"));
        assert_eq!(notification.desktop_entry().as_deref(), Some("thunderbird"));
        assert_eq!(notification.position_hint(), Some((10, 20)));
    }

    #[test]
    fn integer_urgency_is_accepted() {
        let mut hints = HashMap::new();
        hints.insert(HINT_URGENCY.to_string(), value(0i32));
        assert_eq!(Urgency::from_hints(&hints), Urgency::Low);

        hints.insert(HINT_URGENCY.to_string(), value(7i32));
        assert_eq!(Urgency::from_hints(&hints), Urgency::Normal);
    }

    #[test]
    fn labels_lose_control_characters_and_limits() {
        assert_eq!(sanitize_label("  hello\nworld  "), "hello world");
        let long = "x".repeat(MAX_LABEL_CHARS + 20);
        assert_eq!(sanitize_label(&long).chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn bodies_keep_markup_and_newlines() {
        assert_eq!(
            sanitize_body("hello\u{0} <b>world</b>\nnext"),
            "hello <b>world</b>\nnext"
        );
    }

    #[test]
    fn the_default_settings_match_the_spec() {
        let settings = NotificationSettings::default();

        assert!(!settings.ignore_timeout);
        assert!(!settings.dont_disturb);
        assert_eq!(settings.default_timeout, -1);
        assert_eq!(settings.max_visible, 3);
        assert_eq!(settings.display_timeout, 5000);
        assert_eq!(settings.position, Corner::TopRight);
        assert_eq!(settings.monitor_policy, MonitorPolicy::Farthest);
        assert_eq!(settings.server_capabilities.len(), 9);
    }
}
