//! The `menus` builtin. It lists every menu that is not hidden.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::{menu_entry_item, MenuSource, SourceEnv, SourceError};
use crate::menu::model::{MenuDef, MenuItem};
use crate::menu::query::filter_and_sort;

pub struct MenusSource {
    menus: Arc<BTreeMap<String, MenuDef>>,
}

impl MenusSource {
    pub fn new(env: Arc<SourceEnv>) -> Self {
        Self {
            menus: Arc::clone(&env.menus),
        }
    }
}

#[async_trait]
impl MenuSource for MenusSource {
    async fn query(&self, query: &str) -> Result<Vec<MenuItem>, SourceError> {
        let mut entries: Vec<(&String, &MenuDef)> =
            self.menus.iter().filter(|(_, def)| !def.hidden).collect();
        entries.sort_by(|(left_id, left), (right_id, right)| {
            left.title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left_id.cmp(right_id))
        });

        let items: Vec<MenuItem> = entries
            .into_iter()
            .map(|(id, def)| menu_entry_item(id, def))
            .collect();
        Ok(filter_and_sort(&items, query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::MenuIcon;
    use crate::menu::source::MENU_ICON;
    use crate::menu::usage::Usage;

    fn source() -> MenusSource {
        let config = crate::menu::config::parse_menus(
            r#"
            schema = 1

            [menu.alpha]
            title = "Alpha"
            hidden = true

            [menu.alpha.source]
            type = "builtin"
            name = "static"

            [menu.beta]
            title = "Beta"

            [menu.beta.source]
            type = "builtin"
            name = "static"

            [menu.zeta]
            title = "Zeta"
            icon = "preferences-desktop-theme-symbolic"

            [menu.zeta.source]
            type = "builtin"
            name = "static"
            "#,
        )
        .unwrap();

        let env = SourceEnv {
            home: std::path::PathBuf::new(),
            config_dir: std::path::PathBuf::new(),
            data_dirs: Vec::new(),
            menus: Arc::new(config.menus),
            usage: Arc::new(Usage::empty()),
        };
        MenusSource::new(Arc::new(env))
    }

    #[tokio::test]
    async fn lists_visible_menus_in_title_order() {
        let items = source().query("").await.unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "menu-beta");
        assert_eq!(items[0].name, "Beta");
        assert_eq!(items[0].icon, Some(MenuIcon::Named(MENU_ICON.to_string())));
        assert_eq!(items[1].id, "menu-zeta");
        assert_eq!(
            items[1].icon,
            Some(MenuIcon::Named(
                "preferences-desktop-theme-symbolic".to_string()
            ))
        );

        let action = items[1].action.as_ref().unwrap();
        assert_eq!(
            &action.command[1..],
            &["menu".to_string(), "zeta".to_string()]
        );
    }
}
