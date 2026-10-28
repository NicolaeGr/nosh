//! One implementation of the session actions.
//!
//! logind is the only backend. The trait keeps the service testable without a
//! system bus.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::BackendError;
use crate::power::model::{PowerAction, PowerModel};

#[async_trait]
pub trait PowerBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn probe(&self) -> Result<(), BackendError>;

    /// Reads which actions the system accepts.
    async fn read(&self) -> Result<PowerModel, BackendError>;

    /// Sends the action. The call returns when logind accepts it, not when the
    /// machine has finished the transition.
    async fn execute(&self, action: PowerAction) -> Result<(), BackendError>;
}

pub type PowerBackendRef = Arc<dyn PowerBackend>;
