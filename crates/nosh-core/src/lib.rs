//! System state for the nosh shell.
//!
//! This crate has no GTK or display server dependency. It talks to the
//! compositor, resolves backends, and publishes typed events over a channel.
//! The UI crate renders snapshots and sends commands.

pub mod backends;
pub mod backoff;
pub mod bridge;
pub mod command;
pub mod error;
pub mod event;
pub mod hyprland;
pub mod model;
pub mod paths;
pub mod resolver;
pub mod workspaces;

pub use backends::{BackendEvent, BackendRef, WorkspaceBackend};
pub use bridge::{Core, CoreSettings};
pub use command::Command;
pub use error::{BackendError, CoreError, EventParseError, PathError};
pub use event::{Event, Service, Snapshot, WorkspaceEvent};
pub use model::{MonitorModel, WorkspaceCapabilities, WorkspaceModel, WorkspaceSnapshot};
pub use paths::{HyprlandPaths, NoshPaths};
pub use resolver::{Candidate, Resolved};
