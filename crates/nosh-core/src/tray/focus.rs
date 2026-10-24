//! Window focus for tray activation.
//!
//! A click reads the compositor's client list instead of counting clicks. If a
//! window matches, focus it; otherwise let the item's `Activate` call launch it.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::error::{BackendError, PathError};
use crate::hyprland::client::HyprlandClient;
use crate::hyprland::dispatch::{detect_provider, format_dispatch, ConfigProvider, Dispatch};
use crate::hyprland::json::{HyprClient, HyprStatus, HyprVersion, HyprWorkspace};
use crate::paths::HyprlandPaths;
use crate::tray::model::TrayItemModel;

/// The identity a tray item offers when looking for its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowHint {
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub bus_name: String,
    /// The process that owns the item's bus name, when it can be read.
    pub pid: Option<u32>,
}

impl WindowHint {
    pub fn from_model(item: &TrayItemModel, pid: Option<u32>) -> Self {
        Self {
            id: item.id.clone(),
            title: item.title.clone(),
            icon_name: item.icon_name.clone(),
            bus_name: item.bus_name.clone(),
            pid,
        }
    }
}

/// Focuses an existing window for a tray item.
#[async_trait]
pub trait WindowFocus: Send + Sync {
    /// Returns `true` when a window was focused. `false` means the caller falls
    /// back to `Activate`.
    async fn focus(&self, hint: &WindowHint) -> Result<bool, BackendError>;
}

pub type WindowFocusRef = Arc<dyn WindowFocus>;

/// Finds the window that belongs to an item.
///
/// Tiers, most reliable first. A lower tier always wins, and inside a tier the
/// most recently focused window is chosen:
///
/// 1. The process id that owns the bus name.
/// 2. The window class equals the item id.
/// 3. The window class equals the icon name.
/// 4. The window class equals the last segment of the well-known bus name.
/// 5. The window title equals the item title.
///
/// Hidden and unmapped windows are ignored.
pub fn pick_window<'a>(clients: &'a [HyprClient], hint: &WindowHint) -> Option<&'a HyprClient> {
    let mut best: Option<(u8, i32, &HyprClient)> = None;

    for client in clients {
        if !client.mapped || client.hidden {
            continue;
        }
        let Some(tier) = match_tier(client, hint) else {
            continue;
        };
        let better = match best {
            None => true,
            Some((best_tier, best_focus, _)) => {
                tier < best_tier || (tier == best_tier && client.focus_history_id > best_focus)
            }
        };
        if better {
            best = Some((tier, client.focus_history_id, client));
        }
    }

    best.map(|(_, _, client)| client)
}

fn match_tier(client: &HyprClient, hint: &WindowHint) -> Option<u8> {
    if let Some(pid) = hint.pid.filter(|pid| *pid > 0) {
        if client.pid == pid {
            return Some(0);
        }
    }

    let class = client_class(client);
    if !class.is_empty() {
        if same_app(class, &hint.id) {
            return Some(1);
        }
        if hint
            .icon_name
            .as_deref()
            .is_some_and(|name| same_app(class, name))
        {
            return Some(2);
        }
        if bus_app_name(&hint.bus_name).is_some_and(|name| same_app(class, name)) {
            return Some(3);
        }
    }

    if !hint.title.is_empty() && client.title.eq_ignore_ascii_case(&hint.title) {
        return Some(4);
    }

    None
}

fn client_class(client: &HyprClient) -> &str {
    if client.class.is_empty() {
        &client.initial_class
    } else {
        &client.class
    }
}

fn on_active_workspace(client: &HyprClient, active: Option<&HyprWorkspace>) -> bool {
    match (client.workspace.as_ref(), active) {
        (Some(workspace), Some(active)) => workspace.id == active.id,
        _ => false,
    }
}

/// Compares two application names, ignoring case and the `.desktop` suffix.
fn same_app(left: &str, right: &str) -> bool {
    let left = strip_desktop(left.trim());
    let right = strip_desktop(right.trim());
    !left.is_empty() && left.eq_ignore_ascii_case(right)
}

fn strip_desktop(value: &str) -> &str {
    value.strip_suffix(".desktop").unwrap_or(value)
}

/// The last segment of a well-known bus name. `:1.42` gives `None`.
fn bus_app_name(bus_name: &str) -> Option<&str> {
    if bus_name.starts_with(':') {
        return None;
    }
    bus_name
        .rsplit('.')
        .next()
        .filter(|segment| !segment.is_empty())
}

/// Focuses a Hyprland client that matches a tray item.
pub struct HyprlandFocus {
    client: HyprlandClient,
    provider: RwLock<Option<ConfigProvider>>,
}

impl HyprlandFocus {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn detect() -> Result<Option<Self>, PathError> {
        Ok(HyprlandPaths::from_env()?.map(Self::new))
    }

    pub fn new(paths: HyprlandPaths) -> Self {
        Self {
            client: HyprlandClient::new(paths.command_socket),
            provider: RwLock::new(None),
        }
    }

    pub fn from_env() -> Option<Self> {
        match Self::detect() {
            Ok(focus) => focus,
            Err(error) => {
                tracing::debug!(%error, "cannot resolve the Hyprland focus backend");
                None
            }
        }
    }

    async fn provider(&self) -> ConfigProvider {
        if let Some(provider) = *self.provider.read().await {
            return provider;
        }

        let status: Option<HyprStatus> = self.client.query_json("j/status").await.ok();
        let version: Option<HyprVersion> = self.client.query_json("j/version").await.ok();
        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            version.as_ref().map(|version| version.tag.as_str()),
        );
        *self.provider.write().await = Some(provider);
        provider
    }

    async fn focus_client(&self, address: &str) -> Result<(), BackendError> {
        let provider = self.provider().await;
        let request = format_dispatch(provider, Dispatch::FocusClient(address));
        match self.client.query(&request).await {
            Ok(_) => Ok(()),
            Err(first_error) => {
                // The provider can change after a compositor restart or a config
                // reload. Detect again once.
                *self.provider.write().await = None;
                let provider = self.provider().await;
                let request = format_dispatch(provider, Dispatch::FocusClient(address));
                self.client
                    .query(&request)
                    .await
                    .map(|_| ())
                    .map_err(|_| first_error)
            }
        }
    }
}

#[async_trait]
impl WindowFocus for HyprlandFocus {
    async fn focus(&self, hint: &WindowHint) -> Result<bool, BackendError> {
        let clients: Vec<HyprClient> = self.client.query_json("j/clients").await?;
        let Some(target) = pick_window(&clients, hint) else {
            return Ok(false);
        };

        // A window on another workspace is left to `Activate`, which marks that
        // workspace urgent. Focusing it here would switch to it.
        let active: Option<HyprWorkspace> = self.client.query_json("j/activeworkspace").await.ok();
        if !on_active_workspace(target, active.as_ref()) {
            return Ok(false);
        }

        self.focus_client(&target.address).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(address: &str, class: &str) -> HyprClient {
        HyprClient {
            address: address.to_string(),
            mapped: true,
            class: class.to_string(),
            initial_class: class.to_string(),
            pid: 100,
            focus_history_id: 1,
            ..HyprClient::default()
        }
    }

    fn hint() -> WindowHint {
        WindowHint {
            id: "keepassxc".to_string(),
            title: "KeePassXC".to_string(),
            icon_name: Some("keepassxc".to_string()),
            bus_name: "org.kde.keepassxc".to_string(),
            pid: None,
        }
    }

    #[test]
    fn the_process_id_is_the_strongest_match() {
        let mut by_class = client("0x1", "keepassxc");
        by_class.pid = 200;
        let mut by_pid = client("0x2", "something-else");
        by_pid.pid = 42;

        let mut hint = hint();
        hint.pid = Some(42);

        assert_eq!(
            pick_window(&[by_class, by_pid], &hint).map(|client| client.address.as_str()),
            Some("0x2")
        );
    }

    #[test]
    fn the_class_matches_the_item_id() {
        let clients = vec![client("0x1", "Other"), client("0x2", "KeePassXC")];
        assert_eq!(
            pick_window(&clients, &hint()).map(|client| client.address.as_str()),
            Some("0x2")
        );
    }

    #[test]
    fn a_desktop_suffix_is_ignored_on_either_side() {
        let clients = vec![client("0x1", "keepassxc.desktop")];
        assert!(pick_window(&clients, &hint()).is_some());

        let mut hint = hint();
        hint.id = "keepassxc.desktop".to_string();
        let clients = vec![client("0x1", "keepassxc")];
        assert!(pick_window(&clients, &hint).is_some());
    }

    #[test]
    fn the_icon_name_and_bus_name_are_fallbacks() {
        let mut hint = hint();
        hint.id = "unrelated".to_string();
        let clients = vec![client("0x1", "keepassxc")];
        assert_eq!(
            pick_window(&clients, &hint).map(|client| client.address.as_str()),
            Some("0x1")
        );

        hint.icon_name = None;
        let clients = vec![client("0x1", "KeePassXC")];
        assert!(pick_window(&clients, &hint).is_some());
    }

    #[test]
    fn a_unique_bus_name_is_not_treated_as_an_application_name() {
        let mut hint = hint();
        hint.id = "unrelated".to_string();
        hint.icon_name = None;
        hint.bus_name = ":1.42".to_string();
        let clients = vec![client("0x1", "42")];
        assert!(pick_window(&clients, &hint).is_none());
    }

    #[test]
    fn the_most_recently_focused_window_wins_inside_a_tier() {
        let mut old = client("0x1", "keepassxc");
        old.focus_history_id = 3;
        let mut recent = client("0x2", "keepassxc");
        recent.focus_history_id = 9;

        let clients = vec![old, recent];
        assert_eq!(
            pick_window(&clients, &hint()).map(|client| client.address.as_str()),
            Some("0x2")
        );
    }

    #[test]
    fn hidden_and_unmapped_windows_are_skipped() {
        let mut hidden = client("0x1", "keepassxc");
        hidden.hidden = true;
        let mut unmapped = client("0x2", "keepassxc");
        unmapped.mapped = false;

        assert!(pick_window(&[hidden, unmapped], &hint()).is_none());
    }

    #[test]
    fn the_title_is_the_last_resort() {
        let mut hint = hint();
        hint.id = "unrelated".to_string();
        hint.icon_name = None;
        hint.bus_name = "com.example.other".to_string();
        hint.title = "Notes".to_string();

        let clients = vec![client("0x1", "unknown").with_title("notes")];
        assert_eq!(
            pick_window(&clients, &hint).map(|client| client.address.as_str()),
            Some("0x1")
        );
    }

    #[test]
    fn no_candidate_means_no_focus() {
        let clients = vec![client("0x1", "unrelated")];
        assert!(pick_window(&clients, &hint()).is_none());
    }

    fn workspace(id: i64, name: &str) -> HyprWorkspace {
        HyprWorkspace {
            id,
            name: name.to_string(),
            ..HyprWorkspace::default()
        }
    }

    #[test]
    fn only_a_window_on_the_active_workspace_is_focused() {
        let mut target = client("0x1", "keepassxc");
        target.workspace = Some(workspace(2, "2"));
        let active = workspace(2, "2");
        assert!(on_active_workspace(&target, Some(&active)));

        let other = workspace(3, "3");
        assert!(!on_active_workspace(&target, Some(&other)));
    }

    #[test]
    fn an_unknown_workspace_falls_back_to_activate() {
        let mut target = client("0x1", "keepassxc");
        target.workspace = None;
        let active = workspace(2, "2");

        assert!(!on_active_workspace(&target, Some(&active)));
        assert!(!on_active_workspace(&target, None));
    }

    trait WithTitle {
        fn with_title(self, title: &str) -> Self;
    }

    impl WithTitle for HyprClient {
        fn with_title(mut self, title: &str) -> Self {
            self.title = title.to_string();
            self
        }
    }
}
