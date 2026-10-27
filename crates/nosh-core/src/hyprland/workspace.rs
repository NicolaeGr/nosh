//! Hyprland workspace backend: `j/workspaces` reads, shared event updates, and
//! workspace dispatches.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Mutex;

use crate::error::{BackendError, PathError};
use crate::hyprland::connection::{Hyprland, HyprlandState};
use crate::hyprland::dispatch::{format_dispatch, Dispatch};
use crate::hyprland::events::HyprlandEvent;
use crate::hyprland::json::{build_workspace_state, HyprClient, HyprMonitor, HyprWorkspace};
use crate::paths::HyprlandPaths;
use crate::workspaces::backend::{WorkspaceBackend, WorkspaceBackendEvent};
use crate::workspaces::model::WorkspaceCapabilities;
use crate::workspaces::state::WorkspaceState;

pub struct HyprlandWorkspaceBackend {
    hypr: Arc<Hyprland>,
    state: Mutex<WorkspaceState>,
    dispatch_failure_logged: AtomicBool,
}

impl HyprlandWorkspaceBackend {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn detect() -> Result<Option<Self>, PathError> {
        Ok(Hyprland::detect()?.map(Self::with_connection))
    }

    pub fn new(paths: HyprlandPaths) -> Self {
        Self::with_connection(Hyprland::connect(paths))
    }

    pub fn with_connection(hypr: Arc<Hyprland>) -> Self {
        Self {
            hypr,
            state: Mutex::new(WorkspaceState::default()),
            dispatch_failure_logged: AtomicBool::new(false),
        }
    }

    pub fn paths(&self) -> &HyprlandPaths {
        self.hypr.paths()
    }

    async fn handle_event(
        &self,
        event: HyprlandEvent,
        events: &Sender<WorkspaceBackendEvent>,
        unknown: &mut HashSet<String>,
    ) -> Result<bool, BackendError> {
        let before = self.state.lock().await.focused_workspace.clone();

        match event {
            HyprlandEvent::Urgent { address } => {
                let workspace = self.state.lock().await.apply_urgent(&address);
                match workspace {
                    Some(workspace) => Ok(events
                        .send(WorkspaceBackendEvent::UrgentChanged {
                            workspace,
                            urgent: true,
                        })
                        .await
                        .is_ok()),
                    None => Ok(true),
                }
            }
            HyprlandEvent::Connected => Ok(true),
            HyprlandEvent::ConfigReloaded => {
                self.hypr.refresh_provider().await;
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
        events: &Sender<WorkspaceBackendEvent>,
        before: Option<String>,
    ) -> Result<bool, BackendError> {
        let state = self.read().await?;
        let focused = state.focused_workspace.clone();

        if events
            .send(WorkspaceBackendEvent::Snapshot(state))
            .await
            .is_err()
        {
            return Ok(false);
        }
        if before != focused
            && events
                .send(WorkspaceBackendEvent::FocusedChanged(focused))
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
        self.hypr.refresh_provider().await;

        Ok(WorkspaceCapabilities::READ
            | WorkspaceCapabilities::WATCH
            | WorkspaceCapabilities::ACTIVATE
            | WorkspaceCapabilities::MOVE
            | WorkspaceCapabilities::URGENT
            | WorkspaceCapabilities::SPECIAL)
    }

    async fn read(&self) -> Result<WorkspaceState, BackendError> {
        let monitors: Vec<HyprMonitor> = self.hypr.query_json("j/monitors").await?;
        let workspaces: Vec<HyprWorkspace> = self.hypr.query_json("j/workspaces").await?;
        let clients: Vec<HyprClient> = match self.hypr.query_json("j/clients").await {
            Ok(clients) => clients,
            Err(error) => {
                tracing::debug!(%error, "Hyprland client query failed; urgency is degraded");
                Vec::new()
            }
        };
        let active: Option<HyprWorkspace> = match self.hypr.query_json("j/activeworkspace").await {
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
        let provider = self.hypr.provider().await;
        let request = format_dispatch(provider, Dispatch::FocusWorkspace(workspace));
        let response = self.hypr.request(&request).await?;
        self.note_dispatch_response(&request, &response);
        self.state
            .lock()
            .await
            .set_focused(Some(workspace.to_string()));
        Ok(())
    }

    async fn move_to_monitor(&self, workspace: &str, monitor: &str) -> Result<(), BackendError> {
        let provider = self.hypr.provider().await;
        let request = format_dispatch(
            provider,
            Dispatch::MoveWorkspaceToMonitor { workspace, monitor },
        );
        let response = self.hypr.request(&request).await?;
        self.note_dispatch_response(&request, &response);
        Ok(())
    }

    async fn watch(&self, events: Sender<WorkspaceBackendEvent>) -> Result<(), BackendError> {
        let mut hypr_events = self.hypr.events();
        let mut state = self.hypr.state();
        let mut unknown = HashSet::new();

        loop {
            if *state.borrow_and_update() == HyprlandState::Lost {
                return Err(BackendError::Unavailable(
                    "Hyprland event socket lost".to_string(),
                ));
            }

            tokio::select! {
                changed = state.changed() => {
                    if changed.is_err() {
                        return Err(BackendError::Unavailable(
                            "Hyprland event socket lost".to_string(),
                        ));
                    }
                }
                received = hypr_events.recv() => {
                    match received {
                        Ok(event) => {
                            if !self.handle_event(event, &events, &mut unknown).await? {
                                return Ok(());
                            }
                        }
                        Err(RecvError::Lagged(_)) => {
                            tracing::debug!("workspace event stream lagged; resyncing");
                            let before = self.state.lock().await.focused_workspace.clone();
                            if !self.refresh_and_emit(&events, before).await? {
                                return Ok(());
                            }
                        }
                        Err(RecvError::Closed) => {
                            return Err(BackendError::Unavailable(
                                "Hyprland event socket lost".to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }
}
