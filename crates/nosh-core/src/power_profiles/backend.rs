//! One implementation of power profile reading and writing.
//!
//! `power-profiles-daemon` is the only backend. The trait keeps the service
//! testable without a system bus.

use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::power_profiles::model::PowerProfile;
use crate::power_profiles::model::PowerProfilesModel;

/// A change reported by a running backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerProfilesBackendEvent {
    /// One of the daemon properties changed.
    Changed,
    /// The daemon came back. The next read is a fresh snapshot.
    Reconnected,
}

/// Reads and writes the active power profile.
#[async_trait]
pub trait PowerProfilesBackend: Send + Sync {
    fn name(&self) -> &'static str;

    async fn probe(&self) -> Result<(), BackendError>;

    async fn read(&self) -> Result<PowerProfilesModel, BackendError>;

    async fn set(&self, profile: &PowerProfile) -> Result<(), BackendError>;

    /// Runs until the properties stream ends or the receiver closes.
    async fn watch(&self, events: Sender<PowerProfilesBackendEvent>) -> Result<(), BackendError>;
}

pub type PowerProfilesBackendRef = Arc<dyn PowerProfilesBackend>;
