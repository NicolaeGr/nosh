/// User intent. Commands are cheap to send and never block the caller.
///
/// Variants are added with the feature that needs them. Workspaces come first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    WorkspaceActivate(String),
    WorkspaceMoveToMonitor { workspace: String, monitor: String },
}
