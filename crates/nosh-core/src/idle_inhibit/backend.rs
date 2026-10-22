//! The idle inhibitor backend contract.
//!
//! A backend opens one connection to the compositor and reports when the
//! protocol becomes available, when the inhibitor changes, and when the
//! connection is lost. The service owns the desired state and reconnects.

use async_channel::Receiver;

use crate::error::BackendError;

/// A request sent to a live inhibitor connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitCommand {
    Set(bool),
}

/// An event reported by a live inhibitor connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InhibitBackendEvent {
    /// The connection is up. `available` is true when the protocol is bound and
    /// the inhibitor surface exists.
    Connected { available: bool, active: bool },
    /// The protocol appeared or disappeared on the live connection.
    Availability(bool),
    /// An inhibitor was created or destroyed.
    ActiveChanged(bool),
    /// The connection ended and must be rebuilt.
    Lost(String),
}

/// A live connection to the compositor's idle inhibit protocol.
pub trait InhibitSession: Send + Sync {
    /// Requests a new active state. The result arrives as
    /// [`InhibitBackendEvent::ActiveChanged`] when the compositor accepted the
    /// request.
    fn set_active(&self, active: bool) -> Result<(), BackendError>;

    /// Events from this connection.
    fn events(&self) -> Receiver<InhibitBackendEvent>;
}

/// Opens idle inhibitor connections.
pub trait IdleInhibitBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Connects and starts the inhibitor worker. `initial_active` is applied
    /// before the first [`InhibitBackendEvent::Connected`] event.
    fn connect(&self, initial_active: bool) -> Result<Box<dyn InhibitSession>, BackendError>;
}
