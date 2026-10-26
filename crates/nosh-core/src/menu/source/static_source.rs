//! The `static` builtin. The configuration holds the complete item list.

use async_trait::async_trait;
use serde::Deserialize;

use super::{MenuSource, SourceError};
use crate::menu::model::MenuItem;
use crate::menu::query::filter_and_sort;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticOptions {
    #[serde(default)]
    items: Vec<MenuItem>,
}

pub struct StaticSource {
    items: Vec<MenuItem>,
}

impl StaticSource {
    pub fn from_options(options: Option<&toml::Value>) -> Result<Self, SourceError> {
        let Some(value) = options else {
            return Ok(Self { items: Vec::new() });
        };
        let options: StaticOptions =
            value
                .clone()
                .try_into()
                .map_err(|error: toml::de::Error| SourceError::Options {
                    builtin: "static".to_string(),
                    message: error.to_string(),
                })?;
        Ok(Self {
            items: options.items,
        })
    }
}

#[async_trait]
impl MenuSource for StaticSource {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError> {
        Ok(filter_and_sort(&self.items, query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{After, MenuPreview, MenuValue};

    #[test]
    fn parses_static_items() {
        let value: toml::Value = toml::from_str(
            r#"
            [[items]]
            id = "lock"
            name = "Lock"
            description = "Lock the session"
            icon = "system-lock-screen"
            preview = { text = "A short note" }
            action = { command = ["swaylock"] }

            [[items]]
            id = "reboot"
            name = "Reboot"
            action = { command = ["systemctl", "reboot"], after = "none" }
            "#,
        )
        .unwrap();

        let source = StaticSource::from_options(Some(&value)).unwrap();
        assert_eq!(source.items.len(), 2);
        assert_eq!(source.items[0].name, "Lock");
        assert_eq!(
            source.items[0].preview,
            Some(MenuPreview::Text {
                text: "A short note".to_string(),
            })
        );
        assert_eq!(source.items[1].action.as_ref().unwrap().after, After::None);
    }

    #[test]
    fn static_fields_are_kept() {
        let value: toml::Value = toml::from_str(
            r#"
            [[items]]
            id = "one"
            name = "One"
            fields = { path = "/tmp/a.png", count = 3 }
            action = { command = ["true"] }
            "#,
        )
        .unwrap();
        let source = StaticSource::from_options(Some(&value)).unwrap();
        assert_eq!(
            source.items[0].fields.get("path"),
            Some(&MenuValue::String("/tmp/a.png".to_string()))
        );
    }

    #[tokio::test]
    async fn static_items_are_filtered() {
        let value: toml::Value = toml::from_str(
            r#"
            [[items]]
            id = "one"
            name = "One"
            action = { command = ["true"] }

            [[items]]
            id = "two"
            name = "Two"
            action = { command = ["true"] }
            "#,
        )
        .unwrap();
        let source = StaticSource::from_options(Some(&value)).unwrap();
        let items = source.query("two").await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Two");
    }
}
