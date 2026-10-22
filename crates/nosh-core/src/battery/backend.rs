//! One implementation of battery reading and watching.
//!
//! UPower is the only backend. The trait and candidate list keep the service
//! testable without a system bus.

use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;

use crate::battery::model::BatteryProperties;
use crate::error::BackendError;

/// A change reported by a running backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryBackendEvent {
    /// One of the display device properties changed.
    Changed,
    /// The service that owns the device came back. The next read is a fresh
    /// snapshot, not an incremental change.
    Reconnected,
}

/// Reads the display device and reports changes.
#[async_trait]
pub trait BatteryBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Must not read or write anything outside the display device.
    async fn probe(&self) -> Result<(), BackendError>;

    async fn read(&self) -> Result<BatteryProperties, BackendError>;

    /// Runs until the properties stream ends or the receiver closes.
    async fn watch(&self, events: Sender<BatteryBackendEvent>) -> Result<(), BackendError>;
}

pub type BatteryBackendRef = Arc<dyn BatteryBackend>;

type BackendFactory = Arc<dyn Fn() -> Result<BatteryBackendRef, BackendError> + Send + Sync>;

#[derive(Clone)]
pub struct BatteryCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl BatteryCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> Result<BatteryBackendRef, BackendError> + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> Result<BatteryBackendRef, BackendError> {
        (self.make)()
    }
}

pub struct ResolvedBattery {
    pub backend: BatteryBackendRef,
    pub name: &'static str,
}

/// Tries battery candidates in order and returns the first that probes.
pub async fn resolve_battery(
    feature: &'static str,
    candidates: Vec<BatteryCandidate>,
) -> Result<ResolvedBattery, BackendError> {
    for candidate in candidates {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "battery backend constructor failed"
                );
                continue;
            }
        };

        match backend.probe().await {
            Ok(()) => {
                tracing::info!(
                    feature,
                    backend = candidate.name,
                    "selected battery backend"
                );
                return Ok(ResolvedBattery {
                    backend,
                    name: candidate.name,
                });
            }
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "battery backend probe failed"
                );
            }
        }
    }

    tracing::debug!(feature, "no battery backend is available");
    Err(BackendError::NoBackend)
}
