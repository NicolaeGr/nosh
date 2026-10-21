pub mod backend;
pub mod client;
pub mod dispatch;
pub mod events;
pub mod json;

pub use backend::HyprlandWorkspaceBackend;
pub use client::HyprlandClient;
pub use dispatch::{ConfigProvider, Dispatch};
pub use events::{parse_event, HyprlandEvent};
pub use json::{
    build_workspace_state, HyprClient, HyprMonitor, HyprStatus, HyprVersion, HyprWorkspace,
};
