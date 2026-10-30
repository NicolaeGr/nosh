//! The `apps` builtin. It reads XDG desktop entries and searches them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use super::{menu_entry_item, MenuSource, SourceEnv, SourceError};
use crate::menu::model::{ActionDef, ItemState, LaunchMode, MenuIcon, MenuItem};
use crate::menu::query::filter_and_sort_with;
use crate::menu::usage::{self, Usage};

pub struct AppsSource {
    env: Arc<SourceEnv>,
    entries: tokio::sync::OnceCell<Vec<MenuItem>>,
}

impl AppsSource {
    pub fn new(env: Arc<SourceEnv>) -> Self {
        Self {
            env,
            entries: tokio::sync::OnceCell::new(),
        }
    }
}

#[async_trait]
impl MenuSource for AppsSource {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError> {
        let items = self.entries.get_or_init(|| load_entries(&self.env)).await;
        let query = query.trim();
        if query.is_empty() {
            return Ok(order_by_usage(items, &self.env.usage));
        }
        let now = usage::now();
        let usage = &self.env.usage;
        Ok(filter_and_sort_with(items, query, |item| {
            (usage.score(&item.id, now) * 4.0).round() as i32
        }))
    }

    fn record_use(&self, item: &MenuItem) {
        self.env.usage.record(&item.id, usage::now());
    }
}

/// Orders the full list by decayed launch score. Apps without history keep
/// their alphabetical order behind the used ones.
fn order_by_usage(items: &[MenuItem], usage: &Usage) -> Vec<MenuItem> {
    let now = usage::now();
    let mut items: Vec<(f64, MenuItem)> = items
        .iter()
        .map(|item| (usage.score(&item.id, now), item.clone()))
        .collect();
    items.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    items.into_iter().map(|(_, item)| item).collect()
}

async fn load_entries(env: &SourceEnv) -> Vec<MenuItem> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();

    for data_dir in &env.data_dirs {
        let directory = data_dir.join("applications");
        let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
            continue;
        };

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(_) => break,
            };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            let Ok(text) = tokio::fs::read_to_string(&path).await else {
                continue;
            };
            if let Some(item) = parse_desktop_entry(&text, &path, id) {
                items.push(item);
            }
        }
    }

    items.extend(custom_entries(env));
    items.extend(menu_entries(env));

    items.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

#[derive(Debug, Default, Deserialize)]
struct LauncherConfig {
    #[serde(default)]
    entry: Vec<CustomEntry>,
}

#[derive(Debug, Deserialize)]
struct CustomEntry {
    name: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    exec: String,
    #[serde(default)]
    terminal: bool,
}

/// Custom entries live in `apps.d/*.toml`. A file may use root-level fields or
/// the `[[entry]]` wrapper, so a multi-entry file still loads. Unknown sections
/// are ignored so provider and applet keys do not break this reader.
fn custom_entries(env: &SourceEnv) -> Vec<MenuItem> {
    let directory = env.config_dir.join("apps.d");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect();
    files.sort();

    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for path in files {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %path.display(),
                    "ignoring custom app entry: cannot read the file"
                );
                continue;
            }
        };
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("entry");
        let mut local = BTreeMap::new();
        for entry in parse_custom_entries(&text, &path) {
            if let Some(item) = custom_item(entry, stem, &mut local, &mut seen) {
                items.push(item);
            }
        }
    }
    items
}

fn parse_custom_entries(text: &str, path: &Path) -> Vec<CustomEntry> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let parsed = match toml::from_str::<LauncherConfig>(text) {
        Ok(config) if !config.entry.is_empty() => Ok(config.entry),
        Ok(_) => toml::from_str::<CustomEntry>(text).map(|entry| vec![entry]),
        Err(error) => Err(error),
    };

    match parsed {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "ignoring custom app entry: file is invalid");
            Vec::new()
        }
    }
}

/// Menus opt into the launcher with `show_in_apps`.
fn menu_entries(env: &SourceEnv) -> Vec<MenuItem> {
    env.menus
        .iter()
        .filter(|(_, def)| def.show_in_apps)
        .map(|(id, def)| menu_entry_item(id, def))
        .collect()
}

/// A duplicate name from another file gets the file stem. A duplicate within
/// one file gets its occurrence count. Anything still clashing gets a numeric
/// suffix, so a messy directory still loads.
fn unique_id(
    name: &str,
    stem: &str,
    local: &mut BTreeMap<String, usize>,
    seen: &mut BTreeSet<String>,
) -> String {
    let slug_name = slug(name);
    let count = {
        let count = local.entry(slug_name.clone()).or_insert(0);
        *count += 1;
        *count
    };

    let mut id = if count > 1 {
        format!("custom-{count}-{slug_name}")
    } else {
        let id = format!("custom-{slug_name}");
        if seen.contains(&id) {
            format!("custom-{}-{slug_name}", slug(stem))
        } else {
            id
        }
    };

    let base = id.clone();
    let mut suffix = 2;
    while !seen.insert(id.clone()) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

fn custom_item(
    entry: CustomEntry,
    stem: &str,
    local: &mut BTreeMap<String, usize>,
    seen: &mut BTreeSet<String>,
) -> Option<MenuItem> {
    let name = entry.name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let id = unique_id(&name, stem, local, seen);
    let mut item = MenuItem::new(id, name);
    item.icon = entry.icon.and_then(|icon| {
        let icon = icon.trim();
        if icon.is_empty() {
            None
        } else if icon.contains('/') {
            Some(MenuIcon::Image {
                image: PathBuf::from(icon),
            })
        } else {
            Some(MenuIcon::Named(icon.to_string()))
        }
    });

    let command = split_exec(&entry.exec);
    if command.is_empty() {
        item.state = ItemState::Disabled {
            reason: "the custom entry has no exec".to_string(),
        };
    } else {
        item.action = Some(ActionDef {
            command,
            keep_open: false,
            after: crate::menu::model::After::Close,
            launch: LaunchMode::Auto,
            terminal: entry.terminal,
        });
    }
    item.keywords = vec![entry.exec];
    item.fields.insert(
        "custom".to_string(),
        crate::menu::model::MenuValue::Bool(true),
    );
    Some(item)
}

fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "entry".to_string()
    } else {
        slug
    }
}

fn parse_desktop_entry(text: &str, path: &Path, id: &str) -> Option<MenuItem> {
    let mut in_desktop_entry = false;
    let mut entry_type = None;
    let mut name = None;
    let mut comment = None;
    let mut icon = None;
    let mut exec = None;
    let mut categories = None;
    let mut no_display = false;
    let mut hidden = false;
    let mut terminal = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();

        match key.trim() {
            "Type" => entry_type = Some(value.to_string()),
            "Name" => name = Some(value.to_string()),
            "Comment" => comment = Some(value.to_string()),
            "Icon" => icon = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "Categories" => categories = Some(value.to_string()),
            "NoDisplay" => no_display = is_true(value),
            "Hidden" => hidden = is_true(value),
            "Terminal" => terminal = is_true(value),
            _ => {}
        }
    }

    if entry_type.as_deref() != Some("Application") || no_display || hidden {
        return None;
    }
    let name = name?.trim().to_string();
    if name.is_empty() {
        return None;
    }

    let mut item = MenuItem::new(id.to_string(), name);
    item.description = comment.clone().filter(|comment| !comment.is_empty());
    item.icon = icon.and_then(|icon| {
        let icon = icon.trim();
        if icon.is_empty() {
            None
        } else if icon.starts_with("file://") {
            Some(MenuIcon::Image {
                image: PathBuf::from(icon.trim_start_matches("file://")),
            })
        } else if icon.contains('/') {
            Some(MenuIcon::Image {
                image: PathBuf::from(icon),
            })
        } else {
            Some(MenuIcon::Named(icon.to_string()))
        }
    });

    let mut keywords = Vec::new();
    if let Some(categories) = categories.as_deref() {
        keywords.extend(
            categories
                .split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToString::to_string),
        );
        item.fields.insert(
            "categories".to_string(),
            crate::menu::model::MenuValue::String(categories.to_string()),
        );
    }
    if let Some(comment) = comment.as_deref() {
        if !comment.is_empty() {
            keywords.push(comment.to_string());
        }
    }
    item.keywords = keywords;

    item.fields.insert(
        "path".to_string(),
        crate::menu::model::MenuValue::String(path.display().to_string()),
    );
    item.fields.insert(
        "desktop_id".to_string(),
        crate::menu::model::MenuValue::String(id.to_string()),
    );
    if let Some(exec) = exec.as_deref() {
        item.fields.insert(
            "exec".to_string(),
            crate::menu::model::MenuValue::String(exec.to_string()),
        );
    }
    item.fields.insert(
        "terminal".to_string(),
        crate::menu::model::MenuValue::Bool(terminal),
    );

    let command = exec.as_deref().map(split_exec).unwrap_or_default();
    if command.is_empty() {
        item.state = ItemState::Disabled {
            reason: "the desktop entry has no Exec".to_string(),
        };
    } else {
        item.action = Some(ActionDef {
            command,
            keep_open: false,
            after: crate::menu::model::After::Close,
            launch: LaunchMode::Auto,
            terminal,
        });
    }

    Some(item)
}

fn is_true(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1")
}

/// Splits an `Exec` line into argv, honoring quotes and backslash escapes, and
/// removing the field codes (`%f`, `%U`, and so on).
fn split_exec(input: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut characters = input.chars().peekable();
    let mut quote = None;
    let mut started = false;

    while let Some(character) = characters.next() {
        match quote {
            Some(quote_character) => {
                if character == '\\' {
                    if let Some(next) = characters.next() {
                        current.push(next);
                    }
                } else if character == quote_character {
                    quote = None;
                } else {
                    current.push(character);
                }
            }
            None => match character {
                ' ' | '\t' => {
                    if started {
                        push_argument(&mut arguments, std::mem::take(&mut current));
                        started = false;
                    }
                }
                '\'' | '"' => {
                    quote = Some(character);
                    started = true;
                }
                '\\' => {
                    if let Some(next) = characters.next() {
                        current.push(next);
                        started = true;
                    }
                }
                _ => {
                    current.push(character);
                    started = true;
                }
            },
        }
    }

    if started {
        push_argument(&mut arguments, current);
    }

    arguments
}

fn push_argument(arguments: &mut Vec<String>, argument: String) {
    if let Some(argument) = strip_field_codes(&argument) {
        arguments.push(argument);
    }
}

fn strip_field_codes(argument: &str) -> Option<String> {
    // A bare field code expands to one or more files and disappears from argv.
    if argument.len() == 2 && argument.starts_with('%') {
        return None;
    }

    let mut output = String::with_capacity(argument.len());
    let mut characters = argument.chars().peekable();
    let mut changed = false;

    while let Some(character) = characters.next() {
        if character != '%' {
            output.push(character);
            continue;
        }

        changed = true;
        match characters.next() {
            Some('%') => output.push('%'),
            Some('f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k') => {}
            Some(other) => {
                output.push('%');
                output.push(other);
            }
            None => output.push('%'),
        }
    }

    if changed && output.trim().is_empty() {
        None
    } else {
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX: &str = r#"
[Desktop Entry]
Type=Application
Name=Firefox
Comment=Browse the web
Icon=firefox
Exec=firefox %u
Categories=Network;WebBrowser;
Terminal=false
"#;

    #[test]
    fn parses_a_desktop_entry() {
        let item = parse_desktop_entry(FIREFOX, Path::new("/tmp/firefox.desktop"), "firefox")
            .expect("entry should parse");

        assert_eq!(item.id, "firefox");
        assert_eq!(item.name, "Firefox");
        assert_eq!(item.description.as_deref(), Some("Browse the web"));
        assert_eq!(item.icon, Some(MenuIcon::Named("firefox".to_string())));
        assert_eq!(
            item.action.map(|action| action.command),
            Some(vec!["firefox".to_string()])
        );
        assert!(item.keywords.contains(&"WebBrowser".to_string()));
    }

    #[test]
    fn skips_hidden_and_non_application_entries() {
        let hidden = FIREFOX.replace("Terminal=false", "Hidden=true");
        assert!(parse_desktop_entry(&hidden, Path::new("/tmp/x.desktop"), "x").is_none());

        let link = FIREFOX.replace("Type=Application", "Type=Link");
        assert!(parse_desktop_entry(&link, Path::new("/tmp/x.desktop"), "x").is_none());

        let nameless = FIREFOX.replace("Name=Firefox", "");
        assert!(parse_desktop_entry(&nameless, Path::new("/tmp/x.desktop"), "x").is_none());
    }

    #[test]
    fn a_terminal_entry_marks_its_action() {
        let text = FIREFOX.replace("Terminal=false", "Terminal=true");
        let item = parse_desktop_entry(&text, Path::new("/tmp/x.desktop"), "x").unwrap();
        assert!(item.action.unwrap().terminal);
    }

    #[test]
    fn a_missing_exec_disables_the_item() {
        let text = FIREFOX.replace("Exec=firefox %u", "");
        let item =
            parse_desktop_entry(&text, Path::new("/tmp/firefox.desktop"), "firefox").unwrap();
        assert!(item.is_disabled());
        assert!(item.action.is_none());
    }

    #[test]
    fn splits_quoted_exec() {
        assert_eq!(
            split_exec(r#"firefox --name "My Browser" 'two words'"#),
            vec!["firefox", "--name", "My Browser", "two words"]
        );
    }

    #[test]
    fn removes_field_codes() {
        assert_eq!(
            split_exec("app --open %f %U --title %c"),
            vec!["app", "--open", "--title"]
        );
        assert_eq!(split_exec("app 100%%"), vec!["app", "100%"]);
    }

    #[test]
    fn handles_backslash_escapes() {
        assert_eq!(split_exec(r#"app one\ two"#), vec!["app", "one two"]);
    }

    #[test]
    fn used_apps_come_first() {
        let usage = Usage::empty();
        usage.record("zulu", usage::now());
        let items = vec![
            MenuItem::new("alpha".to_string(), "Alpha"),
            MenuItem::new("zulu".to_string(), "Zulu"),
            MenuItem::new("mike".to_string(), "Mike"),
        ];

        let ordered = order_by_usage(&items, &usage);
        assert_eq!(ordered[0].id, "zulu");
        assert_eq!(ordered[1].id, "alpha");
        assert_eq!(ordered[2].id, "mike");
    }

    #[test]
    fn reads_custom_entries_from_apps_dir() {
        let directory = tempfile::tempdir().unwrap();
        let config_dir = directory.path().join("nosh");
        let apps_dir = config_dir.join("apps.d");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::write(
            apps_dir.join("browser.toml"),
            r#"
name = "My Browser"
icon = "firefox"
exec = "firefox --new-window"
"#,
        )
        .unwrap();
        std::fs::write(
            apps_dir.join("work.toml"),
            r#"
[[entry]]
name = "My Browser"
exec = "firefox -P work"

[[entry]]
name = "My Browser"
exec = "firefox -P home"
"#,
        )
        .unwrap();

        let env = SourceEnv {
            home: directory.path().to_path_buf(),
            config_dir,
            data_dirs: Vec::new(),
            menus: Arc::new(BTreeMap::new()),
            usage: Arc::new(Usage::empty()),
        };
        let items = custom_entries(&env);

        let ids: Vec<&str> = items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "custom-my-browser",
                "custom-work-my-browser",
                "custom-2-my-browser"
            ]
        );
        assert_eq!(
            items[0].action.as_ref().unwrap().command,
            vec!["firefox", "--new-window"]
        );
        assert!(!items[0].action.as_ref().unwrap().terminal);
    }

    #[test]
    fn a_custom_entry_can_ask_for_a_terminal() {
        let directory = tempfile::tempdir().unwrap();
        let config_dir = directory.path().join("nosh");
        let apps_dir = config_dir.join("apps.d");
        std::fs::create_dir_all(&apps_dir).unwrap();
        std::fs::write(
            apps_dir.join("nvim.toml"),
            r#"
name = "Edit"
exec = "nvim"
terminal = true
"#,
        )
        .unwrap();

        let env = SourceEnv {
            home: directory.path().to_path_buf(),
            config_dir,
            data_dirs: Vec::new(),
            menus: Arc::new(BTreeMap::new()),
            usage: Arc::new(Usage::empty()),
        };
        let items = custom_entries(&env);

        assert!(items[0].action.as_ref().unwrap().terminal);
    }

    #[test]
    fn menus_opt_into_the_launcher() {
        let directory = tempfile::tempdir().unwrap();
        let config = crate::menu::config::parse_menus(
            r#"
            schema = 1

            [menu.themes]
            title = "Themes"
            show_in_apps = true

            [menu.themes.source]
            type = "builtin"
            name = "static"

            [menu.power]
            title = "Power"

            [menu.power.source]
            type = "builtin"
            name = "static"
            "#,
        )
        .unwrap();

        let env = SourceEnv {
            home: directory.path().to_path_buf(),
            config_dir: directory.path().join("nosh"),
            data_dirs: Vec::new(),
            menus: Arc::new(config.menus),
            usage: Arc::new(Usage::empty()),
        };
        let items = menu_entries(&env);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "menu-themes");
        assert_eq!(
            items[0].icon,
            Some(MenuIcon::Named(crate::menu::source::MENU_ICON.to_string()))
        );
    }
}
