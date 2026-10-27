//! Parsing and validation for `menus.toml`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use super::model::{ActionDef, MenuDef, SourceKind, Template, SCHEMA_VERSION};

/// The file written on first start. It is the only source of the default menus.
const MENUS_TEMPLATE: &str = include_str!("menus_template.toml");

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuConfig {
    pub menus: BTreeMap<String, MenuDef>,
}

#[derive(Debug, Error)]
pub enum MenuConfigError {
    #[error("cannot read {path}: {message}")]
    Io { path: String, message: String },
    #[error("cannot parse menu config: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported menus schema {found}, expected {expected}")]
    Schema { found: u32, expected: u32 },
    #[error("menu {id:?} is invalid: {message}")]
    InvalidMenu { id: String, message: String },
    #[error("menu {id:?} is defined in both {first} and {second}")]
    DuplicateMenu {
        id: String,
        first: String,
        second: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MenusFile {
    schema: u32,
    #[serde(default)]
    menu: BTreeMap<String, MenuDef>,
}

/// Loads one menu file. A missing file yields the built-in template.
pub fn load_menus(path: &Path) -> Result<MenuConfig, MenuConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_menus(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default_menus()),
        Err(error) => Err(io_error(path, error)),
    }
}

/// Loads `menus.toml`, creating it from the template when it is missing, then
/// merges every `*.toml` file under `menus.d/` in file name order.
///
/// Duplicate menu ids across files are an error. A drop-in that redefines an
/// existing menu would make the effective configuration depend on file order.
pub fn load_menu_config(config_dir: &Path) -> Result<MenuConfig, MenuConfigError> {
    let main_path = config_dir.join("menus.toml");
    let text = match std::fs::read_to_string(&main_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = std::fs::create_dir_all(config_dir) {
                tracing::warn!(%error, path = %config_dir.display(), "cannot create the config directory");
            }
            match std::fs::write(&main_path, MENUS_TEMPLATE) {
                Ok(()) => tracing::info!(path = %main_path.display(), "wrote the menu template"),
                Err(error) => tracing::warn!(
                    %error,
                    path = %main_path.display(),
                    "cannot write the menu template"
                ),
            }
            MENUS_TEMPLATE.to_string()
        }
        Err(error) => return Err(io_error(&main_path, error)),
    };

    let mut config = parse_menus(&text)?;
    let drop_in_dir = config_dir.join("menus.d");
    if let Err(error) = std::fs::create_dir_all(&drop_in_dir) {
        tracing::warn!(%error, path = %drop_in_dir.display(), "cannot create the menus.d directory");
    }

    let mut files = Vec::new();
    match std::fs::read_dir(&drop_in_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) == Some("toml") {
                    files.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, path = %drop_in_dir.display(), "cannot read the menus.d directory");
        }
    }
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path).map_err(|error| io_error(&path, error))?;
        let drop_in = parse_menus(&text)?;
        for (id, menu) in drop_in.menus {
            if config.menus.contains_key(&id) {
                return Err(MenuConfigError::DuplicateMenu {
                    id,
                    first: main_path.display().to_string(),
                    second: path.display().to_string(),
                });
            }
            config.menus.insert(id, menu);
        }
    }

    Ok(config)
}

fn io_error(path: &Path, error: std::io::Error) -> MenuConfigError {
    MenuConfigError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// Menu ids double as layer-shell names and CSS classes, so keep them to a safe
/// charset.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

pub fn parse_menus(text: &str) -> Result<MenuConfig, MenuConfigError> {
    let file: MenusFile = toml::from_str(text)?;

    if file.schema != SCHEMA_VERSION {
        return Err(MenuConfigError::Schema {
            found: file.schema,
            expected: SCHEMA_VERSION,
        });
    }

    for (id, menu) in &file.menu {
        if !valid_id(id) {
            return Err(MenuConfigError::InvalidMenu {
                id: id.clone(),
                message: "the id must contain only ASCII letters, digits, dashes, and underscores"
                    .to_string(),
            });
        }
        menu.validate()
            .map_err(|message| MenuConfigError::InvalidMenu {
                id: id.clone(),
                message,
            })?;
    }

    Ok(MenuConfig { menus: file.menu })
}

/// The menus that exist without a config file. The template is the single
/// source of truth so the file on disk and the fallback cannot drift.
pub fn default_menus() -> MenuConfig {
    parse_menus(MENUS_TEMPLATE).expect("the built-in menu template is valid")
}

impl MenuDef {
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("title is empty".to_string());
        }

        if self.view.resolved_columns() == 0 {
            return Err("view.columns must be at least 1".to_string());
        }

        match self.source.kind {
            SourceKind::Builtin => {
                let name = self.source.name.as_deref().unwrap_or("").trim();
                if name.is_empty() {
                    return Err("a builtin source needs a name".to_string());
                }
            }
            SourceKind::External => {
                let command = self.source.command.as_deref().unwrap_or(&[]);
                if command.is_empty() || command.iter().any(|word| word.trim().is_empty()) {
                    return Err("an external source needs a non-empty command".to_string());
                }
            }
        }

        if let Some(template) = &self.template {
            validate_template(template)?;
        }
        if let Some(action) = &self.action {
            validate_action("action", action)?;
        }
        for (name, action) in &self.actions {
            validate_action(&format!("actions.{name}"), action)?;
        }
        for (name, action) in &self.context_menu {
            validate_action(&format!("context_menu.{name}"), action)?;
        }

        if let Some(bind) = &self.bind {
            if bind.chord.trim().is_empty() {
                return Err("bind.chord must not be empty".to_string());
            }
        }

        Ok(())
    }
}

fn validate_template(template: &Template) -> Result<(), String> {
    if template.name.trim().is_empty() {
        return Err("template.name is empty".to_string());
    }
    for (label, field) in [
        ("template.id", template.id.as_deref()),
        ("template.description", template.description.as_deref()),
        ("template.badge", template.badge.as_deref()),
        ("template.group", template.group.as_deref()),
        ("template.keywords", template.keywords.as_deref()),
        ("template.action", template.action.as_deref()),
    ] {
        if let Some(field) = field {
            if field.trim().is_empty() {
                return Err(format!("{label} is empty"));
            }
        }
    }
    if let Some(icon) = &template.icon {
        if icon.field.trim().is_empty() {
            return Err("template.icon.field is empty".to_string());
        }
    }
    if let Some(preview) = &template.preview {
        if preview.field.trim().is_empty() {
            return Err("template.preview.field is empty".to_string());
        }
    }
    Ok(())
}

fn validate_action(label: &str, action: &ActionDef) -> Result<(), String> {
    if action.command.is_empty() || action.command.iter().any(|word| word.trim().is_empty()) {
        return Err(format!("{label}.command must contain non-empty words"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{ItemLayout, SearchMode, ViewKind};

    const MINIMAL: &str = r#"
schema = 1

[menu.example]
title = "Example"

[menu.example.source]
type = "builtin"
name = "static"
"#;

    #[test]
    fn minimal_menu_parses_with_defaults() {
        let config = parse_menus(MINIMAL).unwrap();
        let menu = config.menus.get("example").unwrap();

        assert_eq!(menu.title, "Example");
        assert_eq!(menu.view.kind, ViewKind::List);
        assert_eq!(menu.view.item.layout, ItemLayout::Row);
        assert_eq!(menu.view.gap, 8);
        assert_eq!(menu.window.layer, super::super::model::Layer::Overlay);
        assert_eq!(menu.namespace("example"), "nosh-menu-example");
        assert_eq!(menu.width(), 560);
        assert_eq!(menu.source.resolved_search(), SearchMode::Client);
    }

    #[test]
    fn grid_defaults_to_four_columns() {
        let text = r#"
schema = 1

[menu.grid]
title = "Grid"

[menu.grid.view]
type = "grid"

[menu.grid.source]
type = "builtin"
name = "static"
"#;
        let config = parse_menus(text).unwrap();
        let menu = config.menus.get("grid").unwrap();
        assert_eq!(menu.view.resolved_columns(), 4);
        assert_eq!(menu.width(), 800);
    }

    #[test]
    fn external_sources_default_to_source_search() {
        let text = r#"
schema = 1

[menu.ext]
title = "Ext"

[menu.ext.source]
type = "external"
command = ["~/bin/list"]
"#;
        let config = parse_menus(text).unwrap();
        let menu = config.menus.get("ext").unwrap();
        assert_eq!(menu.source.resolved_search(), SearchMode::Source);
        assert_eq!(menu.source.resolved_debounce_ms(), 60);
        assert_eq!(menu.source.resolved_timeout_ms(), 300);
        assert_eq!(menu.source.resolved_limit(), 300);
    }

    #[test]
    fn shell_is_an_alias_for_external() {
        let text = r#"
schema = 1

[menu.ext]
title = "Ext"

[menu.ext.source]
type = "shell"
command = ["list"]
"#;
        let config = parse_menus(text).unwrap();
        assert_eq!(
            config.menus.get("ext").unwrap().source.kind,
            SourceKind::External
        );
    }

    #[test]
    fn missing_schema_is_rejected() {
        let text = r#"
[menu.example]
title = "Example"
[menu.example.source]
type = "builtin"
name = "static"
"#;
        assert!(matches!(
            parse_menus(text).unwrap_err(),
            MenuConfigError::Toml(_)
        ));
    }

    #[test]
    fn wrong_schema_is_rejected() {
        let text = MINIMAL.replace("schema = 1", "schema = 2");
        assert!(matches!(
            parse_menus(&text).unwrap_err(),
            MenuConfigError::Schema { found: 2, .. }
        ));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = MINIMAL.replace("title = \"Example\"", "title = \"Example\"\nnope = true");
        let error = parse_menus(&text).unwrap_err();
        assert!(matches!(error, MenuConfigError::Toml(_)));
    }

    #[test]
    fn builtin_without_a_name_is_rejected() {
        let text = r#"
schema = 1

[menu.example]
title = "Example"

[menu.example.source]
type = "builtin"
"#;
        let error = parse_menus(text).unwrap_err();
        assert!(matches!(error, MenuConfigError::InvalidMenu { id, .. } if id == "example"));
    }

    #[test]
    fn empty_action_is_rejected() {
        let text = r#"
schema = 1

[menu.example]
title = "Example"

[menu.example.source]
type = "builtin"
name = "static"

[menu.example.action]
command = []
"#;
        assert!(parse_menus(text).is_err());
    }

    #[test]
    fn a_menu_with_a_valid_bind_parses() {
        let text = r#"
schema = 1

[menu.apps]
title = "Apps"

[menu.apps.source]
type = "builtin"
name = "apps"

[menu.apps.bind]
chord = "SUPER, Space"
"#;
        let config = parse_menus(text).unwrap();
        let bind = config.menus.get("apps").unwrap().bind.as_ref().unwrap();

        assert_eq!(bind.chord, "SUPER, Space");
        assert!(!bind.forced);
    }

    #[test]
    fn an_empty_bind_chord_is_rejected() {
        let text = r#"
schema = 1

[menu.apps]
title = "Apps"

[menu.apps.source]
type = "builtin"
name = "apps"

[menu.apps.bind]
chord = "  "
"#;
        let error = parse_menus(text).unwrap_err();

        assert!(matches!(error, MenuConfigError::InvalidMenu { id, message }
                if id == "apps" && message.contains("bind.chord")));
    }

    #[test]
    fn defaults_contain_the_apps_menu() {
        let config = default_menus();
        let apps = config.menus.get("apps").unwrap();
        assert_eq!(apps.source.name.as_deref(), Some("apps"),);
        assert_eq!(apps.source.resolved_search(), SearchMode::Source);
        assert_eq!(apps.view.kind, ViewKind::Grid);
        assert_eq!(apps.view.resolved_columns(), 2);
        assert_eq!(apps.view.item.layout, ItemLayout::Row);
    }

    #[test]
    fn the_template_is_valid_and_written_when_missing() {
        let directory = tempfile::tempdir().unwrap();
        let config = load_menu_config(directory.path()).unwrap();

        let path = directory.path().join("menus.toml");
        assert!(path.exists());
        assert!(directory.path().join("menus.d").is_dir());
        assert!(config.menus.contains_key("apps"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), MENUS_TEMPLATE);
    }

    #[test]
    fn drop_ins_are_merged_in_file_name_order() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("menus.d")).unwrap();
        std::fs::write(directory.path().join("menus.toml"), "schema = 1\n").unwrap();
        std::fs::write(
            directory.path().join("menus.d/20-power.toml"),
            r#"
schema = 1

[menu.power]
title = "Power"

[menu.power.source]
type = "builtin"
name = "static"
"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("menus.d/10-wallpapers.toml"),
            r#"
schema = 1

[menu.wallpapers]
title = "Wallpapers"

[menu.wallpapers.source]
type = "builtin"
name = "files"

[menu.wallpapers.source.options]
path = "/tmp"
"#,
        )
        .unwrap();

        let config = load_menu_config(directory.path()).unwrap();
        assert_eq!(config.menus.len(), 2);
        assert!(config.menus.contains_key("power"));
        assert!(config.menus.contains_key("wallpapers"));
    }

    #[test]
    fn duplicate_ids_across_files_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("menus.d")).unwrap();
        std::fs::write(
            directory.path().join("menus.d/apps.toml"),
            r#"
schema = 1

[menu.apps]
title = "Apps again"

[menu.apps.source]
type = "builtin"
name = "apps"
"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("menus.toml"),
            r#"schema = 1

[menu.apps]
title = "Apps"

[menu.apps.source]
type = "builtin"
name = "apps"
"#,
        )
        .unwrap();

        let error = load_menu_config(directory.path()).unwrap_err();
        assert!(matches!(error, MenuConfigError::DuplicateMenu { id, .. } if id == "apps"));
    }

    #[test]
    fn ids_with_path_separators_are_rejected() {
        let text = r#"
schema = 1

[menu."../evil"]
title = "Evil"

[menu."../evil".source]
type = "builtin"
name = "static"
"#;
        let error = parse_menus(text).unwrap_err();
        assert!(matches!(error, MenuConfigError::InvalidMenu { .. }));
    }
}
