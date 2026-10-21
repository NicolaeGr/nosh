use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::model::WorkspaceCapabilities;
use crate::workspaces::state::WorkspaceState;

/// A change produced by a running workspace backend.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    Snapshot(WorkspaceState),
    FocusedChanged(Option<String>),
    UrgentChanged { workspace: String, urgent: bool },
}

/// One implementation of workspace reading and writing.
///
/// A backend is selected by the resolver. `probe` must not change state.
#[async_trait]
pub trait WorkspaceBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn probe(&self) -> Result<WorkspaceCapabilities, BackendError>;

    async fn read(&self) -> Result<WorkspaceState, BackendError>;

    async fn activate(&self, workspace: &str) -> Result<(), BackendError>;

    async fn move_to_monitor(&self, workspace: &str, monitor: &str) -> Result<(), BackendError>;

    /// Runs until the connection fails or the event receiver closes.
    async fn watch(&self, events: Sender<BackendEvent>) -> Result<(), BackendError>;
}

pub type BackendRef = std::sync::Arc<dyn WorkspaceBackend>;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_channel::Sender;
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;

    pub struct FakeWorkspaceBackend {
        pub name: &'static str,
        pub probe_result: Result<WorkspaceCapabilities, BackendError>,
        pub read_result: Result<WorkspaceState, BackendError>,
        pub watch_result: Result<(), BackendError>,
        pub watch_delay: Option<std::time::Duration>,
        pub watches: AtomicUsize,
        pub probe_failures: AtomicUsize,
        pub state: Mutex<WorkspaceState>,
    }

    impl FakeWorkspaceBackend {
        pub fn new(name: &'static str, capabilities: WorkspaceCapabilities) -> Self {
            Self {
                name,
                probe_result: Ok(capabilities),
                read_result: Ok(WorkspaceState::default()),
                watch_result: Err(BackendError::Unavailable("watch ended".to_string())),
                watch_delay: None,
                watches: AtomicUsize::new(0),
                probe_failures: AtomicUsize::new(0),
                state: Mutex::new(WorkspaceState::default()),
            }
        }

        pub fn watch_count(&self) -> usize {
            self.watches.load(Ordering::SeqCst)
        }

        pub fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl WorkspaceBackend for FakeWorkspaceBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn probe(&self) -> Result<WorkspaceCapabilities, BackendError> {
            if self.probe_failures.load(Ordering::SeqCst) > 0 {
                self.probe_failures.fetch_sub(1, Ordering::SeqCst);
                return Err(BackendError::Unavailable("probe failed".to_string()));
            }
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<WorkspaceState, BackendError> {
            match &self.read_result {
                Ok(state) => Ok(state.clone()),
                Err(error) => Err(error.clone()),
            }
        }

        async fn activate(&self, workspace: &str) -> Result<(), BackendError> {
            self.state
                .lock()
                .await
                .set_focused(Some(workspace.to_string()));
            Ok(())
        }

        async fn move_to_monitor(
            &self,
            _workspace: &str,
            _monitor: &str,
        ) -> Result<(), BackendError> {
            Ok(())
        }

        async fn watch(&self, _events: Sender<BackendEvent>) -> Result<(), BackendError> {
            self.watches.fetch_add(1, Ordering::SeqCst);
            if let Some(delay) = self.watch_delay {
                tokio::time::sleep(delay).await;
            }
            self.watch_result.clone()
        }
    }
}
