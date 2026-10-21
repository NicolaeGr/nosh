use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

use crate::backends::{BackendEvent, WorkspaceBackend};
use crate::error::{BackendError, PathError};
use crate::hyprland::client::HyprlandClient;
use crate::hyprland::dispatch::{detect_provider, format_dispatch, ConfigProvider, Dispatch};
use crate::hyprland::events::{parse_event, HyprlandEvent};
use crate::hyprland::json::{
    build_workspace_state, HyprClient, HyprMonitor, HyprStatus, HyprVersion, HyprWorkspace,
};
use crate::model::WorkspaceCapabilities;
use crate::paths::HyprlandPaths;
use crate::workspaces::state::WorkspaceState;

const EVENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct HyprlandWorkspaceBackend {
    paths: HyprlandPaths,
    client: HyprlandClient,
    state: Mutex<WorkspaceState>,
    provider: RwLock<Option<ConfigProvider>>,
    dispatch_failure_logged: AtomicBool,
}

impl HyprlandWorkspaceBackend {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn detect() -> Result<Option<Self>, PathError> {
        Ok(HyprlandPaths::from_env()?.map(Self::new))
    }

    pub fn new(paths: HyprlandPaths) -> Self {
        let client = HyprlandClient::new(paths.command_socket.clone());
        Self {
            paths,
            client,
            state: Mutex::new(WorkspaceState::default()),
            provider: RwLock::new(None),
            dispatch_failure_logged: AtomicBool::new(false),
        }
    }

    pub fn paths(&self) -> &HyprlandPaths {
        &self.paths
    }

    async fn provider(&self) -> ConfigProvider {
        if let Some(provider) = *self.provider.read().await {
            return provider;
        }

        let provider = self.query_provider().await;
        *self.provider.write().await = Some(provider);
        provider
    }

    async fn query_provider(&self) -> ConfigProvider {
        let status: Option<HyprStatus> = match self.client.query_json("j/status").await {
            Ok(status) => Some(status),
            Err(error) => {
                tracing::debug!(%error, "Hyprland status query failed");
                None
            }
        };
        let version: Option<HyprVersion> = match self.client.query_json("j/version").await {
            Ok(version) => Some(version),
            Err(error) => {
                tracing::debug!(%error, "Hyprland version query failed");
                None
            }
        };

        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            version.as_ref().map(|version| version.tag.as_str()),
        );
        tracing::debug!(?provider, "detected Hyprland config provider");
        provider
    }

    async fn refresh_provider(&self) {
        let provider = self.query_provider().await;
        *self.provider.write().await = Some(provider);
    }

    async fn handle_event(
        &self,
        event: HyprlandEvent,
        events: &Sender<BackendEvent>,
        unknown: &mut HashSet<String>,
    ) -> Result<bool, BackendError> {
        let before = self.state.lock().await.focused_workspace.clone();

        match event {
            HyprlandEvent::Urgent { address } => {
                let workspace = self.state.lock().await.apply_urgent(&address);
                match workspace {
                    Some(workspace) => Ok(events
                        .send(BackendEvent::UrgentChanged {
                            workspace,
                            urgent: true,
                        })
                        .await
                        .is_ok()),
                    None => Ok(true),
                }
            }
            HyprlandEvent::ConfigReloaded => {
                self.refresh_provider().await;
                self.refresh_and_emit(events, before).await
            }
            HyprlandEvent::Submap { .. }
            | HyprlandEvent::ActiveLayout { .. }
            | HyprlandEvent::ActiveWindow { .. } => Ok(true),
            HyprlandEvent::Unknown { name, .. } => {
                if unknown.insert(name.clone()) {
                    tracing::debug!(event = %name, "ignoring unsupported Hyprland event");
                }
                Ok(true)
            }
            _ => self.refresh_and_emit(events, before).await,
        }
    }

    async fn refresh_and_emit(
        &self,
        events: &Sender<BackendEvent>,
        before: Option<String>,
    ) -> Result<bool, BackendError> {
        let state = self.read().await?;
        let focused = state.focused_workspace.clone();

        if events.send(BackendEvent::Snapshot(state)).await.is_err() {
            return Ok(false);
        }
        if before != focused
            && events
                .send(BackendEvent::FocusedChanged(focused))
                .await
                .is_err()
        {
            return Ok(false);
        }

        Ok(true)
    }

    fn note_dispatch_response(&self, request: &str, response: &str) {
        if response.trim().eq_ignore_ascii_case("ok") {
            self.dispatch_failure_logged.store(false, Ordering::Relaxed);
        } else if !self.dispatch_failure_logged.swap(true, Ordering::Relaxed) {
            tracing::warn!(request, response = %response, "Hyprland rejected a dispatch");
        }
    }
}

#[async_trait]
impl WorkspaceBackend for HyprlandWorkspaceBackend {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    async fn probe(&self) -> Result<WorkspaceCapabilities, BackendError> {
        let version: HyprVersion = self.client.query_json("j/version").await?;
        let status: Option<HyprStatus> = match self.client.query_json("j/status").await {
            Ok(status) => Some(status),
            Err(error) => {
                tracing::debug!(%error, "Hyprland status query failed during probe");
                None
            }
        };

        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            Some(version.tag.as_str()),
        );
        *self.provider.write().await = Some(provider);

        Ok(WorkspaceCapabilities::READ
            | WorkspaceCapabilities::WATCH
            | WorkspaceCapabilities::ACTIVATE
            | WorkspaceCapabilities::MOVE
            | WorkspaceCapabilities::URGENT
            | WorkspaceCapabilities::SPECIAL)
    }

    async fn read(&self) -> Result<WorkspaceState, BackendError> {
        let monitors: Vec<HyprMonitor> = self.client.query_json("j/monitors").await?;
        let workspaces: Vec<HyprWorkspace> = self.client.query_json("j/workspaces").await?;
        let clients: Vec<HyprClient> = match self.client.query_json("j/clients").await {
            Ok(clients) => clients,
            Err(error) => {
                tracing::debug!(%error, "Hyprland client query failed; urgency is degraded");
                Vec::new()
            }
        };
        let active: Option<HyprWorkspace> = match self.client.query_json("j/activeworkspace").await
        {
            Ok(active) => Some(active),
            Err(error) => {
                tracing::debug!(%error, "Hyprland active workspace query failed");
                None
            }
        };

        let (workspaces, monitors, clients, focused) =
            build_workspace_state(&monitors, &workspaces, active.as_ref(), &clients);

        let mut state = self.state.lock().await;
        state.replace(workspaces, monitors, clients, focused);
        Ok(state.clone())
    }

    async fn activate(&self, workspace: &str) -> Result<(), BackendError> {
        let provider = self.provider().await;
        let request = format_dispatch(provider, Dispatch::FocusWorkspace(workspace));
        let response = self.client.query(&request).await?;
        self.note_dispatch_response(&request, &response);
        self.state
            .lock()
            .await
            .set_focused(Some(workspace.to_string()));
        Ok(())
    }

    async fn move_to_monitor(&self, workspace: &str, monitor: &str) -> Result<(), BackendError> {
        let provider = self.provider().await;
        let request = format_dispatch(
            provider,
            Dispatch::MoveWorkspaceToMonitor { workspace, monitor },
        );
        let response = self.client.query(&request).await?;
        self.note_dispatch_response(&request, &response);
        Ok(())
    }

    async fn watch(&self, events: Sender<BackendEvent>) -> Result<(), BackendError> {
        let stream = timeout(
            EVENT_CONNECT_TIMEOUT,
            UnixStream::connect(&self.paths.event_socket),
        )
        .await
        .map_err(|_| {
            BackendError::Unavailable(format!(
                "timed out connecting to {}",
                self.paths.event_socket.display()
            ))
        })?
        .map_err(|error| {
            BackendError::Unavailable(format!(
                "cannot connect to {}: {error}",
                self.paths.event_socket.display()
            ))
        })?;

        let mut lines = BufReader::new(stream).lines();
        let mut unknown = HashSet::new();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => {
                    return Err(BackendError::Unavailable(
                        "Hyprland event socket closed".to_string(),
                    ));
                }
                Err(error) => {
                    return Err(BackendError::Request(format!(
                        "cannot read the Hyprland event socket: {error}"
                    )));
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            let event = match parse_event(&line) {
                Ok(event) => event,
                Err(error) => {
                    tracing::debug!(%error, line, "ignoring malformed Hyprland event");
                    continue;
                }
            };

            if !self.handle_event(event, &events, &mut unknown).await? {
                return Ok(());
            }
        }
    }
}
