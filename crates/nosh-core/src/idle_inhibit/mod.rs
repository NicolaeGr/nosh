//! Idle inhibition through `zwp_idle_inhibit_manager_v1`.
//!
//! This is an inhibitor, not an idle notifier. The service keeps a surface on
//! the compositor alive and attaches an inhibitor to it to prevent idle. It
//! does not report when the user becomes idle.

pub mod backend;
pub mod model;
pub mod service;
pub mod wayland;

pub use backend::{IdleInhibitBackend, InhibitBackendEvent, InhibitCommand, InhibitSession};
pub use model::IdleInhibitState;
pub use service::{IdleInhibitService, IdleInhibitServiceConfig};
pub use wayland::WaylandBackend;
