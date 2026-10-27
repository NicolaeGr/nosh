pub mod binds;
pub mod client;
pub mod connection;
pub mod dispatch;
pub mod events;
pub mod json;
pub mod monitor;
pub mod workspace;

pub use binds::{BindError, BindPlan, BindSpec, BindSyncOutcome, Chord};
pub use client::HyprlandClient;
pub use connection::{Hyprland, HyprlandState};
pub use dispatch::{ConfigProvider, Dispatch};
pub use events::{parse_event, HyprlandEvent};
pub use json::{
    build_workspace_state, HyprBind, HyprClient, HyprMonitor, HyprStatus, HyprVersion,
    HyprWorkspace,
};
pub use monitor::HyprlandMonitorBackend;
pub use workspace::HyprlandWorkspaceBackend;
