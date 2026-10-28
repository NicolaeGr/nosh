//! One implementation of the logind session lock watch.
//!
//! The trait keeps the service testable without a system bus.

use std::sync::Arc;

use async_channel::Sender;

use crate::error::BackendError;
use crate::event::SessionLockEvent;

/// A source of logind lock requests.
#[async_trait::async_trait]
pub trait SessionLockBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Checks that the caller's session object can be resolved.
    async fn probe(&self) -> Result<(), BackendError>;

    /// Forwards lock requests until the receiver closes or the connection
    /// fails. The service sends `Unavailable` before reporting an error.
    async fn watch(&self, events: Sender<SessionLockEvent>) -> Result<(), BackendError>;
}

pub type SessionLockBackendRef = Arc<dyn SessionLockBackend>;
