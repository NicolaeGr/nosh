//! The DBusMenu client.
//!
//! Turns `GetLayout` into a [`TrayMenu`] and applies `ItemsPropertiesUpdated`
//! changes. Every call has a timeout, so a slow item cannot block the core or
//! the GTK thread.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use zbus::zvariant::{OwnedValue, Structure, Type, Value};

use crate::error::BackendError;
use crate::tray::model::{item_from_properties, TrayMenu, TrayMenuItem};

pub const DBUSMENU_INTERFACE: &str = "com.canonical.dbusmenu";

/// The properties requested from `GetLayout`.
pub const WANTED_PROPERTIES: [&str; 9] = [
    "label",
    "enabled",
    "visible",
    "type",
    "children-display",
    "toggle-type",
    "toggle-state",
    "icon-name",
    "shortcut",
];

/// The deepest menu nesting accepted from an item.
pub const MAX_MENU_DEPTH: usize = 32;

/// The largest number of menu entries accepted from one item.
pub const MAX_MENU_ITEMS: usize = 2048;

/// The default timeout for one DBusMenu call.
pub const MENU_TIMEOUT: Duration = Duration::from_secs(2);

/// The layout node as DBusMenu sends it. `children` are variants that each
/// contain another `(ia{sv}av)` structure.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct RawLayoutNode {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

/// A client for one item's `com.canonical.dbusmenu` object.
pub struct DbusMenuClient {
    proxy: zbus::Proxy<'static>,
    timeout: Duration,
}

impl DbusMenuClient {
    pub async fn new(
        connection: &zbus::Connection,
        bus_name: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<Self, BackendError> {
        let proxy = zbus::Proxy::new_owned(
            connection.clone(),
            bus_name.to_string(),
            path.to_string(),
            DBUSMENU_INTERFACE,
        )
        .await
        .map_err(|error| BackendError::Unavailable(error.to_string()))?;

        Ok(Self { proxy, timeout })
    }

    pub fn proxy(&self) -> &zbus::Proxy<'static> {
        &self.proxy
    }

    /// Asks the item to prepare its menu. Errors and timeouts are ignored.
    pub async fn about_to_show(&self) {
        let body = (0i32,);
        let call = self.proxy.call_method("AboutToShow", &body);
        if tokio::time::timeout(self.timeout, call).await.is_err() {
            tracing::debug!("a tray menu AboutToShow call timed out");
        }
    }

    /// Reads the full menu. `None` means the call failed or timed out, so the
    /// UI can fall back to `Activate`.
    pub async fn layout(&self) -> Option<TrayMenu> {
        let wanted: &[&str] = &WANTED_PROPERTIES;
        let body = (0i32, -1i32, wanted);
        let call = self
            .proxy
            .call::<_, _, (u32, RawLayoutNode)>("GetLayout", &body);

        match tokio::time::timeout(self.timeout, call).await {
            Ok(Ok((revision, root))) => Some(menu_from_root(revision, root)),
            Ok(Err(error)) => {
                tracing::debug!(%error, "a tray menu layout call failed");
                None
            }
            Err(_) => {
                tracing::debug!("a tray menu layout call timed out");
                None
            }
        }
    }

    /// Sends one menu event. Failures are logged and dropped.
    pub async fn send_event(&self, menu_id: i32, event: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .unwrap_or(0);
        let data = Value::from("");
        let body = (menu_id, event, data, timestamp);
        let call = self.proxy.call_method("Event", &body);

        match tokio::time::timeout(self.timeout, call).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::debug!(%error, menu_id, event, "a tray menu event failed"),
            Err(_) => tracing::debug!(menu_id, event, "a tray menu event timed out"),
        }
    }
}

/// Builds a menu from the root layout node.
pub(crate) fn menu_from_root(revision: u32, root: RawLayoutNode) -> TrayMenu {
    let mut count = 0usize;
    let items = root
        .children
        .into_iter()
        .filter_map(node_from_variant)
        .filter_map(|node| item_from_node(node, 1, &mut count))
        .collect();

    TrayMenu { revision, items }
}

fn node_from_variant(value: OwnedValue) -> Option<RawLayoutNode> {
    // A child arrives as a variant that wraps the layout structure. Unwrap the
    // nested variants before reading the fields.
    let mut value = Value::from(value);
    while let Value::Value(inner) = value {
        value = *inner;
    }

    let structure = Structure::try_from(&value).ok()?;
    let fields = structure.fields();
    if fields.len() != 3 {
        return None;
    }

    let id = i32::try_from(fields[0].try_clone().ok()?).ok()?;
    let properties =
        HashMap::<String, OwnedValue>::try_from(OwnedValue::try_from(&fields[1]).ok()?).ok()?;
    let children = Vec::<OwnedValue>::try_from(OwnedValue::try_from(&fields[2]).ok()?).ok()?;

    Some(RawLayoutNode {
        id,
        properties,
        children,
    })
}

fn item_from_node(node: RawLayoutNode, depth: usize, count: &mut usize) -> Option<TrayMenuItem> {
    if depth > MAX_MENU_DEPTH || *count >= MAX_MENU_ITEMS {
        tracing::debug!("a tray menu exceeded the accepted size");
        return None;
    }
    *count += 1;

    let children = node
        .children
        .into_iter()
        .filter_map(node_from_variant)
        .filter_map(|child| item_from_node(child, depth + 1, count))
        .collect();

    Some(item_from_properties(node.id, &node.properties, children))
}

/// Parses a `GetLayout` reply body without a proxy. Exposed for tests.
#[cfg(test)]
pub(crate) fn menu_from_layout(revision: u32, root: RawLayoutNode) -> TrayMenu {
    menu_from_root(revision, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::model::{TrayMenuItemKind, TrayToggleState, TrayToggleType};

    fn value<T: Into<Value<'static>>>(value: T) -> OwnedValue {
        OwnedValue::try_from(value.into()).unwrap()
    }

    fn properties(entries: &[(&str, OwnedValue)]) -> HashMap<String, OwnedValue> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn a_new_menu_keeps_labels_enabled_and_kind() {
        let root = RawLayoutNode {
            id: 0,
            properties: HashMap::new(),
            children: Vec::new(),
        };

        // The root itself is not rendered, so an empty root is an empty menu.
        assert_eq!(
            menu_from_layout(7, root),
            TrayMenu {
                revision: 7,
                items: Vec::new(),
            }
        );
    }

    #[test]
    fn menu_item_properties_map_to_the_model() {
        let mut map = properties(&[
            ("label", value("Open")),
            ("enabled", value(true)),
            ("visible", value(true)),
            ("type", value("standard")),
            ("toggle-type", value("checkmark")),
            ("toggle-state", value(1i32)),
            ("icon-name", value("folder")),
        ]);

        let item = item_from_properties(4, &map, Vec::new());
        assert_eq!(item.id, 4);
        assert_eq!(item.label, "Open");
        assert!(item.enabled);
        assert!(item.visible);
        assert_eq!(item.kind, TrayMenuItemKind::Standard);
        assert_eq!(item.toggle_type, Some(TrayToggleType::Checkmark));
        assert_eq!(item.toggle_state, TrayToggleState::On);
        assert_eq!(item.icon_name.as_deref(), Some("folder"));

        map.insert("type".to_string(), value("separator"));
        map.insert("toggle-type".to_string(), value("radio"));
        map.insert("toggle-state".to_string(), value(2i32));
        let item = item_from_properties(5, &map, Vec::new());
        assert_eq!(item.kind, TrayMenuItemKind::Separator);
        assert_eq!(item.toggle_type, Some(TrayToggleType::Radio));
        assert_eq!(item.toggle_state, TrayToggleState::Indeterminate);
    }

    #[test]
    fn a_property_update_changes_only_present_fields() {
        let mut menu = TrayMenu {
            revision: 1,
            items: vec![TrayMenuItem {
                id: 1,
                label: "Old".to_string(),
                enabled: true,
                visible: true,
                kind: TrayMenuItemKind::Standard,
                toggle_type: None,
                toggle_state: TrayToggleState::Off,
                icon_name: None,
                shortcut: Vec::new(),
                children: Vec::new(),
            }],
        };

        let changed = menu.apply_updates(&[(
            1,
            properties(&[("label", value("New")), ("enabled", value(false))]),
        )]);
        assert!(changed);
        let item = menu.find(1).unwrap();
        assert_eq!(item.label, "New");
        assert!(!item.enabled);
        assert!(item.visible);

        let unchanged = menu.apply_updates(&[(2, properties(&[("label", value("Other"))]))]);
        assert!(!unchanged);
    }

    #[test]
    fn a_removed_property_returns_to_its_default() {
        use crate::tray::model::TrayMenuItemKind;

        let mut menu = TrayMenu {
            revision: 1,
            items: vec![TrayMenuItem {
                id: 1,
                label: "Old".to_string(),
                enabled: false,
                visible: false,
                kind: TrayMenuItemKind::Separator,
                toggle_type: Some(TrayToggleType::Checkmark),
                toggle_state: TrayToggleState::On,
                icon_name: Some("folder".to_string()),
                shortcut: vec!["Ctrl+O".to_string()],
                children: Vec::new(),
            }],
        };

        let removed: Vec<String> = [
            "label",
            "enabled",
            "visible",
            "type",
            "toggle-type",
            "toggle-state",
            "icon-name",
            "shortcut",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect();

        assert!(menu.apply_removed(&[(1, removed)]));

        let item = menu.find(1).unwrap();
        assert_eq!(item.label, "");
        assert!(item.enabled);
        assert!(item.visible);
        assert_eq!(item.kind, TrayMenuItemKind::Standard);
        assert_eq!(item.toggle_type, None);
        assert_eq!(item.toggle_state, TrayToggleState::Off);
        assert_eq!(item.icon_name, None);
        assert!(item.shortcut.is_empty());
    }

    #[test]
    fn a_layout_reply_with_nested_children_deserializes() {
        use zbus::zvariant::{serialized::Context, to_bytes, LE};

        let leaf = (
            1i32,
            properties(&[("label", value("Quit"))]),
            Vec::<OwnedValue>::new(),
        );
        let leaf_variant =
            OwnedValue::try_from(Value::new(Value::Structure(Structure::from(leaf)))).unwrap();
        let root = (
            0i32,
            HashMap::<String, OwnedValue>::new(),
            vec![leaf_variant],
        );

        let body = (12u32, root);
        let bytes = to_bytes(Context::new_dbus(LE, 0), &body).unwrap();
        let (decoded, _): ((u32, RawLayoutNode), usize) = bytes.deserialize().unwrap();
        let (revision, root) = decoded;

        let menu = menu_from_layout(revision, root);
        assert_eq!(menu.revision, 12);
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].id, 1);
        assert_eq!(menu.items[0].label, "Quit");
    }

    #[test]
    fn wanted_properties_match_the_protocol_list() {
        assert_eq!(
            WANTED_PROPERTIES,
            [
                "label",
                "enabled",
                "visible",
                "type",
                "children-display",
                "toggle-type",
                "toggle-state",
                "icon-name",
                "shortcut",
            ]
        );
    }
}
