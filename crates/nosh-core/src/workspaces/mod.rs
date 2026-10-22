//! Workspace models, backend resolution, and the workspace service.

pub mod backend;
pub mod model;
pub mod resolver;
pub mod service;
pub mod state;

pub use backend::{WorkspaceBackend, WorkspaceBackendEvent, WorkspaceBackendRef};
pub use model::{
    normalize_address, sort_workspaces, WorkspaceCapabilities, WorkspaceModel, WorkspaceMonitor,
    WorkspaceSnapshot,
};
pub use resolver::{resolve_workspace, ResolvedWorkspace, WorkspaceCandidate};
pub use service::{StaticWorkspaceBackend, WorkspaceService, WorkspaceServiceConfig};
pub use state::WorkspaceState;
