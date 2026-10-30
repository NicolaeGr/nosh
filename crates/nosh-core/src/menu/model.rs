//! Menu configuration and the item contract shared by every source.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::keys::TERMINAL_ACTION;

/// The only schema version this build understands.
pub const SCHEMA_VERSION: u32 = 1;

/// The default number of items a source may return per query.
pub const DEFAULT_LIMIT: usize = 300;
/// The default debounce for a source that receives the query.
pub const DEFAULT_DEBOUNCE_MS: u64 = 60;
/// The default timeout for an external source.
pub const DEFAULT_TIMEOUT_MS: u64 = 300;
/// The largest single output line an external source may emit.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// The largest total output an external source may emit.
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemLayout {
    #[default]
    Row,
    Tile,
    Card,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Load once, then nosh filters and sorts.
    #[default]
    Client,
    /// Every debounced query is sent to the source, which owns the order.
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Builtin,
    #[serde(alias = "shell", alias = "script")]
    External,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    #[default]
    Close,
    Reload,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Background,
    Bottom,
    Top,
    #[default]
    Overlay,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keyboard {
    None,
    OnDemand,
    #[default]
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowDef {
    pub namespace: Option<String>,
    pub anchors: Vec<Anchor>,
    pub layer: Layer,
    pub keyboard: Keyboard,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Default for WindowDef {
    fn default() -> Self {
        Self {
            namespace: None,
            anchors: vec![Anchor::Center],
            layer: Layer::default(),
            keyboard: Keyboard::default(),
            width: None,
            height: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemViewDef {
    pub layout: ItemLayout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewDef {
    #[serde(rename = "type")]
    pub kind: ViewKind,
    pub columns: Option<u32>,
    pub cell_width: Option<u32>,
    pub gap: u32,
    pub item: ItemViewDef,
    pub side_preview: bool,
}

impl Default for ViewDef {
    fn default() -> Self {
        Self {
            kind: ViewKind::default(),
            columns: None,
            cell_width: None,
            gap: 8,
            item: ItemViewDef::default(),
            side_preview: false,
        }
    }
}

impl ViewDef {
    /// The column count the UI should use.
    pub fn resolved_columns(&self) -> u32 {
        self.columns.unwrap_or(match self.kind {
            ViewKind::List => 1,
            ViewKind::Grid => 4,
        })
    }

    pub fn resolved_cell_width(&self) -> Option<u32> {
        self.cell_width
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDef {
    #[serde(rename = "type")]
    pub kind: SourceKind,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub options: Option<toml::Value>,
    #[serde(default)]
    pub search: Option<SearchMode>,
    #[serde(default)]
    pub debounce_ms: Option<u64>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl SourceDef {
    pub fn resolved_search(&self) -> SearchMode {
        self.search.unwrap_or(match self.kind {
            SourceKind::Builtin => SearchMode::Client,
            SourceKind::External => SearchMode::Source,
        })
    }

    pub fn resolved_debounce_ms(&self) -> u64 {
        self.debounce_ms.unwrap_or(DEFAULT_DEBOUNCE_MS)
    }

    pub fn resolved_timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)
    }

    pub fn resolved_limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).max(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDef {
    pub command: Vec<String>,
    #[serde(default)]
    pub keep_open: bool,
    #[serde(default)]
    pub after: After,
    #[serde(default)]
    pub launch: LaunchMode,
    /// Start the command inside the menu's terminal wrapper.
    #[serde(default)]
    pub terminal: bool,
}

/// How the action process is started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchMode {
    /// UWSM or a systemd scope, then a direct spawn.
    #[default]
    Auto,
    /// UWSM, then a direct spawn.
    Uwsm,
    /// A systemd user scope, then a direct spawn.
    SystemdRun,
    /// A direct spawn.
    Detached,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconKind {
    #[default]
    Named,
    Image,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewKind {
    #[default]
    Image,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconRef {
    pub field: String,
    #[serde(default)]
    pub kind: IconKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRef {
    pub field: String,
    #[serde(default)]
    pub kind: PreviewKind,
}

/// Maps the fields of an external JSON object onto the item slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<IconRef>,
    #[serde(default)]
    pub preview: Option<PreviewRef>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub keywords: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
}

impl Template {
    /// The mapping used when a menu does not define one.
    pub fn default_fields() -> Self {
        Self {
            id: Some("id".to_string()),
            name: "name".to_string(),
            description: Some("description".to_string()),
            icon: Some(IconRef {
                field: "icon".to_string(),
                kind: IconKind::Named,
            }),
            preview: Some(PreviewRef {
                field: "thumbnail".to_string(),
                kind: PreviewKind::Image,
            }),
            badge: Some("badge".to_string()),
            group: Some("group".to_string()),
            keywords: Some("keywords".to_string()),
            action: Some("action".to_string()),
        }
    }

    /// Builds an item from one JSON object. A missing or empty name skips the
    /// item, because no useful row can be rendered without one.
    pub fn apply(&self, value: &serde_json::Value) -> Result<MenuItem, ItemError> {
        let object = value.as_object().ok_or(ItemError::NotObject)?;

        let name = object
            .get(&self.name)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or(ItemError::MissingName)?
            .to_string();

        let id = self
            .id
            .as_ref()
            .and_then(|field| object.get(field))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or(&name)
            .to_string();

        let description = self
            .description
            .as_ref()
            .and_then(|field| string_field(object, field));
        let badge = self
            .badge
            .as_ref()
            .and_then(|field| string_field(object, field));
        let group = self
            .group
            .as_ref()
            .and_then(|field| string_field(object, field));

        let icon = self.icon.as_ref().and_then(|icon| {
            let value = string_field(object, &icon.field)?;
            Some(match icon.kind {
                IconKind::Named => MenuIcon::Named(value),
                IconKind::Image => MenuIcon::Image {
                    image: PathBuf::from(value),
                },
            })
        });

        let preview = self.preview.as_ref().and_then(|preview| {
            let value = string_field(object, &preview.field)?;
            Some(match preview.kind {
                PreviewKind::Image => MenuPreview::Image {
                    image: PathBuf::from(value),
                },
                PreviewKind::Text => MenuPreview::Text { text: value },
            })
        });

        let mut keywords = Vec::new();
        if let Some(field) = self.keywords.as_ref() {
            match object.get(field) {
                Some(serde_json::Value::String(value)) => keywords.extend(
                    value
                        .split([',', ';'])
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(ToString::to_string),
                ),
                Some(serde_json::Value::Array(values)) => values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .for_each(|value| keywords.push(value.to_string())),
                _ => {}
            }
        }

        let mut fields = BTreeMap::new();
        for (key, value) in object {
            let value = match value {
                serde_json::Value::String(value) => MenuValue::String(value.clone()),
                serde_json::Value::Number(number) => match number.as_f64() {
                    Some(value) => MenuValue::Number(value),
                    None => continue,
                },
                serde_json::Value::Bool(value) => MenuValue::Bool(*value),
                _ => continue,
            };
            fields.insert(key.clone(), value);
        }

        let (action, state) = match self.action.as_ref().and_then(|field| object.get(field)) {
            Some(value) => match action_from_json(value) {
                Ok(action) => (Some(action), ItemState::Normal),
                Err(error) => (
                    None,
                    ItemState::Disabled {
                        reason: error.to_string(),
                    },
                ),
            },
            None => (None, ItemState::Normal),
        };

        Ok(MenuItem {
            id,
            name,
            description,
            icon,
            preview,
            badge,
            group,
            keywords,
            fields,
            action,
            state,
        })
    }
}

fn string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn action_from_json(value: &serde_json::Value) -> Result<ActionDef, ItemError> {
    match value {
        serde_json::Value::Array(words) => {
            let command: Vec<String> = words
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToString::to_string)
                .collect();
            if command.is_empty() {
                return Err(ItemError::BadAction(
                    "action array contains no strings".to_string(),
                ));
            }
            Ok(ActionDef {
                command,
                keep_open: false,
                after: After::default(),
                launch: LaunchMode::default(),
                terminal: false,
            })
        }
        serde_json::Value::Object(_) => serde_json::from_value(value.clone())
            .map_err(|error| ItemError::BadAction(error.to_string())),
        _ => Err(ItemError::BadAction(
            "action must be an argv array or an object with a command array".to_string(),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemError {
    NotObject,
    MissingName,
    BadAction(String),
}

impl std::fmt::Display for ItemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotObject => write!(formatter, "item is not a JSON object"),
            Self::MissingName => write!(formatter, "item has no usable name"),
            Self::BadAction(message) => write!(formatter, "invalid item action: {message}"),
        }
    }
}

impl std::error::Error for ItemError {}

/// One row the UI can render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<MenuIcon>,
    #[serde(default)]
    pub preview: Option<MenuPreview>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, MenuValue>,
    #[serde(default)]
    pub action: Option<ActionDef>,
    #[serde(skip)]
    pub state: ItemState,
}

impl MenuItem {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            icon: None,
            preview: None,
            badge: None,
            group: None,
            keywords: Vec::new(),
            fields: BTreeMap::new(),
            action: None,
            state: ItemState::Normal,
        }
    }

    pub fn field(&self, key: &str) -> Option<String> {
        match self.fields.get(key) {
            Some(MenuValue::String(value)) => Some(value.clone()),
            Some(MenuValue::Number(value)) => Some(format_number(*value)),
            Some(MenuValue::Bool(value)) => Some(value.to_string()),
            None => match key {
                "id" => Some(self.id.clone()),
                "name" => Some(self.name.clone()),
                "description" => self.description.clone(),
                "badge" => self.badge.clone(),
                "group" => self.group.clone(),
                _ => None,
            },
        }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self.state, ItemState::Disabled { .. })
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MenuIcon {
    Image { image: PathBuf },
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MenuPreview {
    Image { image: PathBuf },
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MenuValue {
    Bool(bool),
    Number(f64),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ItemState {
    #[default]
    Normal,
    Disabled {
        reason: String,
    },
}

/// Suffixes duplicate ids so the UI can use them as keys.
pub fn dedupe_ids(items: &mut [MenuItem]) {
    let mut seen = std::collections::HashMap::new();
    for item in items {
        let count = seen.entry(item.id.clone()).or_insert(0usize);
        *count += 1;
        if *count > 1 {
            let base = item.id.clone();
            let mut suffix = *count;
            while seen.contains_key(&format!("{base}-{suffix}")) {
                suffix += 1;
            }
            item.id = format!("{base}-{suffix}");
            seen.insert(item.id.clone(), 1);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuBind {
    pub chord: String,
    #[serde(default)]
    pub forced: bool,
}

/// The argv that starts a terminal emulator. The action argv follows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalDef {
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuDef {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<MenuIcon>,
    /// Skipped by the `menus` builtin. The menu stays reachable by id.
    #[serde(default)]
    pub hidden: bool,
    /// Listed by the `apps` builtin as a launcher entry.
    #[serde(default)]
    pub show_in_apps: bool,
    #[serde(default)]
    pub window: WindowDef,
    #[serde(default)]
    pub view: ViewDef,
    pub source: SourceDef,
    #[serde(default)]
    pub template: Option<Template>,
    #[serde(default)]
    pub action: Option<ActionDef>,
    #[serde(default)]
    pub actions: BTreeMap<String, ActionDef>,
    #[serde(default)]
    pub context_menu: BTreeMap<String, ActionDef>,
    /// Terminal emulator argv used by actions marked `terminal = true` and by
    /// the reserved `terminal` action.
    #[serde(default)]
    pub terminal: Option<TerminalDef>,
    /// In-menu key bindings, as `"ctrl+return" = "action name"`.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub bind: Option<MenuBind>,
}

impl MenuDef {
    pub fn namespace(&self, id: &str) -> String {
        self.window
            .namespace
            .clone()
            .unwrap_or_else(|| format!("nosh-menu-{id}"))
    }

    pub fn width(&self) -> u32 {
        if let Some(width) = self.window.width {
            return width;
        }
        if let (Some(cell_width), columns) = (self.view.cell_width, self.view.resolved_columns()) {
            return cell_width.saturating_mul(columns);
        }
        match self.view.kind {
            ViewKind::List => 560,
            ViewKind::Grid => 800,
        }
    }

    pub fn resolved_template(&self) -> Template {
        self.template
            .clone()
            .unwrap_or_else(Template::default_fields)
    }

    pub fn action_for(&self, item: &MenuItem) -> Option<ActionDef> {
        item.action.clone().or_else(|| self.action.clone())
    }

    /// Resolves the default action or a named alternate.
    ///
    /// The name `terminal` is reserved: it runs the default action inside the
    /// menu terminal. Every other name looks up `actions`, then `context_menu`.
    pub fn action_named(&self, item: &MenuItem, name: Option<&str>) -> Option<ActionDef> {
        match name {
            None => self.action_for(item),
            Some(name) if name == TERMINAL_ACTION => {
                let mut action = self.action_for(item)?;
                action.terminal = true;
                Some(action)
            }
            Some(name) => self
                .actions
                .get(name)
                .or_else(|| self.context_menu.get(name))
                .cloned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn template_maps_the_conventional_fields() {
        let template = Template::default_fields();
        let value = json!({
            "path": "/home/user/Pictures/city.png",
            "name": "City",
            "resolution": "3840x2160",
            "thumbnail": "/tmp/city.png",
            "badge": "4K",
            "width": 3840
        });

        let item = template.apply(&value).unwrap();

        assert_eq!(item.id, "City");
        assert_eq!(item.name, "City");
        assert_eq!(item.badge.as_deref(), Some("4K"));
        assert_eq!(
            item.preview,
            Some(MenuPreview::Image {
                image: PathBuf::from("/tmp/city.png"),
            })
        );
        assert_eq!(
            item.fields.get("path"),
            Some(&MenuValue::String(
                "/home/user/Pictures/city.png".to_string()
            ))
        );
        assert_eq!(item.fields.get("width"), Some(&MenuValue::Number(3840.0)));
        assert!(item.action.is_none());
        assert_eq!(item.state, ItemState::Normal);
    }

    #[test]
    fn template_rejects_items_without_a_name() {
        let template = Template::default_fields();
        assert_eq!(
            template.apply(&json!({"id": "x"})).unwrap_err(),
            ItemError::MissingName
        );
        assert_eq!(
            template.apply(&json!({"name": "  "})).unwrap_err(),
            ItemError::MissingName
        );
        assert_eq!(
            template.apply(&json!(["not", "an", "object"])).unwrap_err(),
            ItemError::NotObject
        );
    }

    #[test]
    fn an_action_can_pick_its_launcher() {
        let action: ActionDef =
            serde_json::from_str(r#"{"command": ["notify-send", "hello"], "launch": "detached"}"#)
                .unwrap();
        assert_eq!(action.launch, LaunchMode::Detached);

        let action: ActionDef = serde_json::from_str(r#"{"command": ["notify-send"]}"#).unwrap();
        assert_eq!(action.launch, LaunchMode::Auto);
    }

    #[test]
    fn template_maps_ids_and_previews_from_custom_fields() {
        let template = Template {
            id: Some("uuid".to_string()),
            name: "title".to_string(),
            description: Some("subtitle".to_string()),
            icon: Some(IconRef {
                field: "glyph".to_string(),
                kind: IconKind::Image,
            }),
            preview: Some(PreviewRef {
                field: "body".to_string(),
                kind: PreviewKind::Text,
            }),
            badge: None,
            group: None,
            keywords: Some("tags".to_string()),
            action: Some("cmd".to_string()),
        };
        let value = json!({
            "uuid": "42",
            "title": "Note",
            "subtitle": "Saved yesterday",
            "glyph": "/tmp/icon.svg",
            "body": "full text",
            "tags": ["one", "two"],
            "cmd": ["notify-send", "{item.title}"]
        });

        let item = template.apply(&value).unwrap();

        assert_eq!(item.id, "42");
        assert_eq!(item.description.as_deref(), Some("Saved yesterday"));
        assert_eq!(
            item.icon,
            Some(MenuIcon::Image {
                image: PathBuf::from("/tmp/icon.svg"),
            })
        );
        assert_eq!(
            item.preview,
            Some(MenuPreview::Text {
                text: "full text".to_string(),
            })
        );
        assert_eq!(item.keywords, vec!["one", "two"]);
        assert_eq!(
            item.action,
            Some(ActionDef {
                command: vec!["notify-send".to_string(), "{item.title}".to_string()],
                keep_open: false,
                after: After::Close,
                launch: LaunchMode::Auto,
                terminal: false,
            })
        );
    }

    #[test]
    fn template_disables_items_with_a_bad_action() {
        let template = Template::default_fields();
        let item = template
            .apply(&json!({"name": "Bad", "action": "not argv"}))
            .unwrap();
        assert!(item.action.is_none());
        assert!(item.is_disabled());
    }

    #[test]
    fn duplicate_ids_are_suffixed() {
        let mut items = vec![
            MenuItem::new("same", "One"),
            MenuItem::new("same", "Two"),
            MenuItem::new("same", "Three"),
        ];
        dedupe_ids(&mut items);
        assert_eq!(items[0].id, "same");
        assert_eq!(items[1].id, "same-2");
        assert_eq!(items[2].id, "same-3");
    }

    #[test]
    fn named_actions_resolve_in_order() {
        let menu: MenuDef = toml::from_str(
            r#"
title = "Test"

[source]
type = "builtin"
name = "static"

[action]
command = ["default"]

[actions.open]
command = ["open"]

[context_menu.ctx]
command = ["ctx"]

[terminal]
command = ["kitty", "--"]
"#,
        )
        .unwrap();
        let item = MenuItem::new("id", "Name");

        assert_eq!(
            menu.action_named(&item, None).unwrap().command,
            vec!["default"]
        );
        assert_eq!(
            menu.action_named(&item, Some("open")).unwrap().command,
            vec!["open"]
        );
        assert_eq!(
            menu.action_named(&item, Some("ctx")).unwrap().command,
            vec!["ctx"]
        );

        let terminal = menu.action_named(&item, Some("terminal")).unwrap();
        assert_eq!(terminal.command, vec!["default"]);
        assert!(terminal.terminal);

        assert!(menu.action_named(&item, Some("nope")).is_none());
    }

    #[test]
    fn width_prefers_the_window_then_the_cell_grid() {
        let mut menu = MenuDef {
            title: "Test".to_string(),
            window: WindowDef::default(),
            view: ViewDef::default(),
            source: SourceDef {
                kind: SourceKind::Builtin,
                name: Some("static".to_string()),
                command: None,
                options: None,
                search: None,
                debounce_ms: None,
                timeout_ms: None,
                limit: None,
            },
            template: None,
            action: None,
            actions: BTreeMap::new(),
            context_menu: BTreeMap::new(),
            terminal: None,
            keys: BTreeMap::new(),
            placeholder: None,
            bind: None,
            description: None,
            icon: None,
            hidden: false,
            show_in_apps: false,
        };

        assert_eq!(menu.width(), 560);
        menu.view.kind = ViewKind::Grid;
        assert_eq!(menu.width(), 800);
        menu.view.cell_width = Some(200);
        assert_eq!(menu.width(), 800);
        menu.window.width = Some(1200);
        assert_eq!(menu.width(), 1200);
    }
}
