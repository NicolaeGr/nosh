//! Tray state as the UI renders it.
//!
//! The core owns every D-Bus object and every menu tree. The UI receives these
//! values and creates widgets, but it never calls D-Bus or parses a menu.

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::zvariant::OwnedValue;

/// The `Status` property of a StatusNotifierItem.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TrayStatus {
    #[default]
    Passive,
    Active,
    NeedsAttention,
}

impl TrayStatus {
    pub fn parse(value: &str) -> Self {
        match value {
            "Active" => Self::Active,
            "NeedsAttention" => Self::NeedsAttention,
            _ => Self::Passive,
        }
    }
}

/// The `Category` property of a StatusNotifierItem.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TrayCategory {
    #[default]
    ApplicationStatus,
    Communications,
    SystemServices,
    Hardware,
}

impl TrayCategory {
    pub fn parse(value: &str) -> Self {
        match value {
            "Communications" => Self::Communications,
            "SystemServices" => Self::SystemServices,
            "Hardware" => Self::Hardware,
            _ => Self::ApplicationStatus,
        }
    }
}

/// A decoded icon pixmap in RGBA order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPixmap {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes in RGBA order.
    pub rgba: Vec<u8>,
}

/// The icon the UI should render for an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayIcon {
    File(PathBuf),
    Pixmap(IconPixmap),
    Named(String),
}

/// One status notifier item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItemModel {
    /// `bus_name + object_path`.
    pub item_id: String,
    pub bus_name: String,
    pub object_path: String,
    pub id: String,
    pub title: String,
    pub status: TrayStatus,
    pub category: TrayCategory,
    pub tooltip_title: String,
    pub tooltip_description: String,
    pub icon_name: Option<String>,
    pub icon_theme_path: Option<PathBuf>,
    pub icon_pixmap: Option<IconPixmap>,
    /// A file resolved from an absolute icon name or from the theme path.
    pub icon_file: Option<PathBuf>,
    pub menu_path: Option<String>,
    pub is_menu: bool,
}

impl TrayItemModel {
    /// The icon to render. A resolved file wins over a theme name, and a name
    /// wins over a pixmap.
    pub fn icon(&self) -> Option<TrayIcon> {
        if let Some(file) = &self.icon_file {
            return Some(TrayIcon::File(file.clone()));
        }
        if let Some(name) = self.icon_name.as_ref().filter(|name| !name.is_empty()) {
            return Some(TrayIcon::Named(name.clone()));
        }
        self.icon_pixmap.clone().map(TrayIcon::Pixmap)
    }

    /// Escaped tooltip markup. Title and description are joined by a newline.
    pub fn tooltip_markup(&self) -> String {
        let title = escape_markup(&self.tooltip_title);
        let description = escape_markup(&self.tooltip_description);
        match (title.is_empty(), description.is_empty()) {
            (true, true) => String::new(),
            (true, false) => description,
            (false, true) => title,
            (false, false) => format!("{title}\n{description}"),
        }
    }
}

fn escape_markup(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// The `type` property of an item. The other value is `standard`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrayMenuItemKind {
    #[default]
    Standard,
    Separator,
}

/// The `toggle-type` property of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayToggleType {
    Checkmark,
    Radio,
}

/// The `toggle-state` property of an item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrayToggleState {
    #[default]
    Off,
    On,
    Indeterminate,
}

impl TrayToggleState {
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::On,
            2 => Self::Indeterminate,
            _ => Self::Off,
        }
    }
}

/// One DBusMenu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub kind: TrayMenuItemKind,
    pub toggle_type: Option<TrayToggleType>,
    pub toggle_state: TrayToggleState,
    pub icon_name: Option<String>,
    pub shortcut: Vec<String>,
    pub children: Vec<TrayMenuItem>,
}

impl TrayMenuItem {
    pub fn is_toggle(&self) -> bool {
        self.toggle_type.is_some()
    }
}

/// The complete menu of one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayMenu {
    pub revision: u32,
    pub items: Vec<TrayMenuItem>,
}

impl TrayMenu {
    pub fn find(&self, id: i32) -> Option<&TrayMenuItem> {
        find_item(&self.items, id)
    }

    pub fn find_mut(&mut self, id: i32) -> Option<&mut TrayMenuItem> {
        find_item_mut(&mut self.items, id)
    }

    /// Applies an `ItemsPropertiesUpdated` payload and reports whether a
    /// rendered field changed.
    pub fn apply_updates(&mut self, updates: &[(i32, HashMap<String, OwnedValue>)]) -> bool {
        let mut changed = false;
        for (id, properties) in updates {
            if let Some(item) = self.find_mut(*id) {
                changed |= apply_properties(item, properties);
            }
        }
        changed
    }

    /// Applies the removed-properties half of `ItemsPropertiesUpdated`. The
    /// return value matches [`Self::apply_updates`].
    pub fn apply_removed(&mut self, removals: &[(i32, Vec<String>)]) -> bool {
        let mut changed = false;
        for (id, properties) in removals {
            if let Some(item) = self.find_mut(*id) {
                changed |= remove_properties(item, properties);
            }
        }
        changed
    }
}

fn find_item(items: &[TrayMenuItem], id: i32) -> Option<&TrayMenuItem> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_item(&item.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_item_mut(items: &mut [TrayMenuItem], id: i32) -> Option<&mut TrayMenuItem> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let Some(found) = find_item_mut(&mut item.children, id) {
            return Some(found);
        }
    }
    None
}

/// Builds one menu entry from the raw DBusMenu property map.
pub(crate) fn item_from_properties(
    id: i32,
    properties: &HashMap<String, OwnedValue>,
    children: Vec<TrayMenuItem>,
) -> TrayMenuItem {
    let mut item = TrayMenuItem {
        id,
        label: String::new(),
        enabled: true,
        visible: true,
        kind: TrayMenuItemKind::Standard,
        toggle_type: None,
        toggle_state: TrayToggleState::Off,
        icon_name: None,
        shortcut: Vec::new(),
        children,
    };
    apply_properties(&mut item, properties);
    item
}

/// Applies a property map to an existing entry. Returns whether a rendered
/// field changed.
pub(crate) fn apply_properties(
    item: &mut TrayMenuItem,
    properties: &HashMap<String, OwnedValue>,
) -> bool {
    let before = item.clone();

    if let Some(label) = get_string(properties, "label") {
        item.label = label;
    }
    if let Some(enabled) = get_bool(properties, "enabled") {
        item.enabled = enabled;
    }
    if let Some(visible) = get_bool(properties, "visible") {
        item.visible = visible;
    }
    if let Some(kind) = get_string(properties, "type") {
        item.kind = if kind == "separator" {
            TrayMenuItemKind::Separator
        } else {
            TrayMenuItemKind::Standard
        };
    }
    if let Some(toggle) = get_string(properties, "toggle-type") {
        item.toggle_type = match toggle.as_str() {
            "checkmark" => Some(TrayToggleType::Checkmark),
            "radio" => Some(TrayToggleType::Radio),
            _ => None,
        };
    }
    if let Some(state) = get_i32(properties, "toggle-state") {
        item.toggle_state = TrayToggleState::from_raw(state);
    }
    if let Some(icon) = get_string(properties, "icon-name") {
        item.icon_name = Some(icon).filter(|icon| !icon.is_empty());
    }
    if let Some(shortcut) = get_strings(properties, "shortcut") {
        item.shortcut = shortcut;
    }

    *item != before
}

fn get_string(values: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    get::<String>(values, key)
}

fn get_bool(values: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    get::<bool>(values, key)
}

fn get_i32(values: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    get::<i32>(values, key)
}

fn get_strings(values: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<String>> {
    get::<Vec<String>>(values, key)
}

/// Clears the properties an item reported as removed, so they fall back to the
/// defaults a menu entry starts with.
fn remove_properties(item: &mut TrayMenuItem, properties: &[String]) -> bool {
    let before = item.clone();

    for property in properties {
        match property.as_str() {
            "label" => item.label.clear(),
            "enabled" => item.enabled = true,
            "visible" => item.visible = true,
            "type" => item.kind = TrayMenuItemKind::Standard,
            "toggle-type" => item.toggle_type = None,
            "toggle-state" => item.toggle_state = TrayToggleState::Off,
            "icon-name" => item.icon_name = None,
            "shortcut" => item.shortcut.clear(),
            _ => {}
        }
    }

    *item != before
}

pub(crate) fn get<T>(values: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    let value = values.get(key)?.try_clone().ok()?;
    T::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> TrayItemModel {
        TrayItemModel {
            item_id: ":1.2/StatusNotifierItem".to_string(),
            bus_name: ":1.2".to_string(),
            object_path: "/StatusNotifierItem".to_string(),
            id: "app".to_string(),
            title: "App".to_string(),
            status: TrayStatus::Active,
            category: TrayCategory::ApplicationStatus,
            tooltip_title: "App".to_string(),
            tooltip_description: "Running".to_string(),
            icon_name: Some("app-icon".to_string()),
            icon_theme_path: None,
            icon_pixmap: None,
            icon_file: None,
            menu_path: None,
            is_menu: false,
        }
    }

    #[test]
    fn status_and_category_parse_known_values_and_default_others() {
        assert_eq!(TrayStatus::parse("Active"), TrayStatus::Active);
        assert_eq!(
            TrayStatus::parse("NeedsAttention"),
            TrayStatus::NeedsAttention
        );
        assert_eq!(TrayStatus::parse("Passive"), TrayStatus::Passive);
        assert_eq!(TrayStatus::parse("unexpected"), TrayStatus::Passive);

        assert_eq!(
            TrayCategory::parse("Communications"),
            TrayCategory::Communications
        );
        assert_eq!(
            TrayCategory::parse("SystemServices"),
            TrayCategory::SystemServices
        );
        assert_eq!(TrayCategory::parse("Hardware"), TrayCategory::Hardware);
        assert_eq!(
            TrayCategory::parse("unexpected"),
            TrayCategory::ApplicationStatus
        );
    }

    #[test]
    fn the_icon_prefers_a_file_then_a_name_then_a_pixmap() {
        let mut model = item();
        model.icon_pixmap = Some(IconPixmap {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        });

        assert_eq!(model.icon(), Some(TrayIcon::Named("app-icon".to_string())));

        model.icon_file = Some(PathBuf::from("/tmp/icon.png"));
        assert_eq!(
            model.icon(),
            Some(TrayIcon::File(PathBuf::from("/tmp/icon.png")))
        );

        model.icon_file = None;
        model.icon_name = None;
        assert_eq!(
            model.icon(),
            Some(TrayIcon::Pixmap(IconPixmap {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            }))
        );
    }

    #[test]
    fn the_tooltip_joins_the_title_and_description() {
        let model = item();
        assert_eq!(model.tooltip_markup(), "App\nRunning");

        let mut model = item();
        model.tooltip_description.clear();
        assert_eq!(model.tooltip_markup(), "App");

        let mut model = item();
        model.tooltip_title.clear();
        assert_eq!(model.tooltip_markup(), "Running");

        let mut model = item();
        model.tooltip_title.clear();
        model.tooltip_description.clear();
        assert_eq!(model.tooltip_markup(), "");
    }

    #[test]
    fn the_tooltip_escapes_glib_markup() {
        let mut model = item();
        model.tooltip_title = "<b>a & b</b>".to_string();
        model.tooltip_description = "\"quoted\"".to_string();
        assert_eq!(
            model.tooltip_markup(),
            "&lt;b&gt;a &amp; b&lt;/b&gt;\n&quot;quoted&quot;"
        );
    }

    #[test]
    fn menu_items_are_found_at_any_depth() {
        let menu = TrayMenu {
            revision: 3,
            items: vec![TrayMenuItem {
                id: 1,
                label: "One".to_string(),
                enabled: true,
                visible: true,
                kind: TrayMenuItemKind::Standard,
                toggle_type: None,
                toggle_state: TrayToggleState::Off,
                icon_name: None,
                shortcut: Vec::new(),
                children: vec![TrayMenuItem {
                    id: 2,
                    label: "Two".to_string(),
                    enabled: true,
                    visible: true,
                    kind: TrayMenuItemKind::Standard,
                    toggle_type: None,
                    toggle_state: TrayToggleState::Off,
                    icon_name: None,
                    shortcut: Vec::new(),
                    children: Vec::new(),
                }],
            }],
        };

        assert_eq!(menu.find(1).map(|item| item.label.as_str()), Some("One"));
        assert_eq!(menu.find(2).map(|item| item.label.as_str()), Some("Two"));
        assert!(menu.find(3).is_none());
    }
}
