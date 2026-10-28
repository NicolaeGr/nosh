//! One implementation of the polkit agent.
//!
//! polkitd is the only backend. The trait keeps the service testable without a
//! system bus.

use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::polkit::model::AgentRequest;

#[async_trait]
pub trait PolkitBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Exports the agent object at `path` and registers it with polkitd.
    ///
    /// Prompts, password requests, and session ends arrive on `requests`.
    async fn register(
        &self,
        path: &str,
        locale: &str,
        requests: Sender<AgentRequest>,
    ) -> Result<(), BackendError>;

    /// Removes the agent object and drops the registration.
    async fn unregister(&self, path: &str) -> Result<(), BackendError>;
}

pub type PolkitBackendRef = Arc<dyn PolkitBackend>;
