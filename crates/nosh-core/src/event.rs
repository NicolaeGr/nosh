use crate::model::WorkspaceSnapshot;

/// Every service publishes state through this enum.
#[derive(Debug, Clone)]
pub enum Event {
    /// Complete state for every service that has started.
    Snapshot(Snapshot),
    Workspaces(WorkspaceEvent),
    ServiceAvailability(Service, bool),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub workspaces: Option<WorkspaceSnapshot>,
    pub workspace_backend: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Service {
    Workspaces,
}

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    /// Fresh state after a resync or reconnect.
    Snapshot {
        state: WorkspaceSnapshot,
        backend: &'static str,
    },
    FocusedChanged {
        workspace: Option<String>,
    },
    UrgentChanged {
        workspace: String,
        urgent: bool,
    },
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}
