//! Lock requests from logind.
//!
//! The shell owns the lock surface, so this module only forwards the `Lock`
//! and `Unlock` signals of the caller's session. It does not lock anything
//! itself.

pub mod backend;
pub mod logind;
pub mod marker;
pub mod service;

pub use backend::{SessionLockBackend, SessionLockBackendRef};
pub use logind::LogindSessionLockBackend;
pub use marker::{LockMarker, LOCK_MARKER_FILE};
pub use service::SessionLockService;
