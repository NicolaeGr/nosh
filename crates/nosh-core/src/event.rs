use crate::model::WorkspaceSnapshot;
use crate::monitors::MonitorLayout;

/// Every service publishes state through this enum.
#[derive(Debug, Clone)]
pub enum Event {
    /// Complete state for every service that has started.
    Snapshot(Snapshot),
    Workspaces(WorkspaceEvent),
    Monitors(MonitorEvent),
    ServiceAvailability(Service, bool),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub workspaces: Option<WorkspaceSnapshot>,
    pub workspace_backend: Option<&'static str>,
    pub monitors: Option<MonitorLayout>,
    pub monitor_backend: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Service {
    Workspaces,
    Monitors,
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

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    /// Fresh state after a resync or reconnect.
    Snapshot {
        layout: MonitorLayout,
        backend: &'static str,
    },
    /// The compositor reported a monitor change. The layout is complete.
    LayoutChanged(MonitorLayout),
    /// A write was rejected or could not be delivered.
    ApplyFailed(String),
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}
