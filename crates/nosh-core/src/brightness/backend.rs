//! The per-device brightness backend trait and its resolver.
//!
//! Brightness resolves per device. An internal panel may use sysfs plus logind
//! while an external display on the same machine uses DDC/CI.

use std::sync::Arc;

use async_trait::async_trait;
use bitflags::bitflags;

use crate::brightness::model::{BrightnessDeviceId, BrightnessDeviceInfo, BrightnessReading};
use crate::error::BrightnessError;

bitflags! {
    /// What a brightness backend can do for a device.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct BrightnessCapabilities: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
    }
}

impl BrightnessCapabilities {
    /// A device the slider can control.
    pub const CONTROL: Self = Self::READ.union(Self::WRITE);
}

/// One implementation of brightness reading and writing.
///
/// A probe must not change a device. Discovery may be slow because it does not
/// run on the UI thread.
#[async_trait]
pub trait BrightnessBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether this backend handles the given identifier.
    fn supports(&self, device: &BrightnessDeviceId) -> bool;

    async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError>;

    /// Lists the devices this backend can control.
    async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError>;

    async fn read(&self, device: &BrightnessDeviceId)
        -> Result<BrightnessReading, BrightnessError>;

    async fn write(&self, device: &BrightnessDeviceId, percent: f64)
        -> Result<(), BrightnessError>;
}

pub type BrightnessBackendRef = Arc<dyn BrightnessBackend>;

type BackendFactory = Arc<dyn Fn() -> Result<BrightnessBackendRef, BrightnessError> + Send + Sync>;

#[derive(Clone)]
pub struct BrightnessCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl BrightnessCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> Result<BrightnessBackendRef, BrightnessError> + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> Result<BrightnessBackendRef, BrightnessError> {
        (self.make)()
    }
}

/// A device bound to a working backend.
#[derive(Clone)]
pub struct ResolvedBrightness {
    pub backend: BrightnessBackendRef,
    pub name: &'static str,
    pub priority: usize,
    pub capabilities: BrightnessCapabilities,
    pub reading: BrightnessReading,
}

/// Tries candidates for one device from most specific to most generic.
pub async fn resolve_device(
    info: &BrightnessDeviceInfo,
    candidates: &[BrightnessCandidate],
) -> Option<ResolvedBrightness> {
    for (priority, candidate) in candidates.iter().enumerate() {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    backend = candidate.name,
                    device = %info.id,
                    %error,
                    "brightness backend constructor failed"
                );
                continue;
            }
        };

        if !backend.supports(&info.id) {
            continue;
        }

        let capabilities = match backend.probe().await {
            Ok(capabilities) if capabilities.contains(BrightnessCapabilities::CONTROL) => {
                capabilities
            }
            Ok(capabilities) => {
                tracing::debug!(
                    backend = candidate.name,
                    device = %info.id,
                    ?capabilities,
                    "brightness backend lacks read or write"
                );
                continue;
            }
            Err(error) => {
                tracing::debug!(
                    backend = candidate.name,
                    device = %info.id,
                    %error,
                    "brightness backend probe failed"
                );
                continue;
            }
        };

        match backend.read(&info.id).await {
            Ok(reading) => {
                tracing::info!(
                    feature = "brightness",
                    backend = candidate.name,
                    device = %info.id,
                    "selected brightness backend"
                );
                return Some(ResolvedBrightness {
                    backend,
                    name: candidate.name,
                    priority,
                    capabilities,
                    reading,
                });
            }
            Err(error) => {
                tracing::debug!(
                    backend = candidate.name,
                    device = %info.id,
                    %error,
                    "brightness device read failed during probing"
                );
            }
        }
    }

    None
}

/// The devices found by one discovery pass and the candidate that found them.
pub struct DiscoveredBrightness {
    /// Index into the candidate list of the backend that reported the devices.
    pub source: usize,
    pub devices: Vec<BrightnessDeviceInfo>,
}

/// Returns the devices from the first candidate that discovers any, along with
/// the index of that candidate. Binding then runs for each device.
pub async fn discover_first(candidates: &[BrightnessCandidate]) -> Option<DiscoveredBrightness> {
    for (source, candidate) in candidates.iter().enumerate() {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    backend = candidate.name,
                    %error,
                    "brightness discovery constructor failed"
                );
                continue;
            }
        };

        if let Err(error) = backend.probe().await {
            tracing::debug!(
                backend = candidate.name,
                %error,
                "brightness discovery probe failed"
            );
            continue;
        }

        match backend.discover().await {
            Ok(devices) if !devices.is_empty() => {
                tracing::debug!(
                    backend = candidate.name,
                    devices = devices.len(),
                    "discovered brightness devices"
                );
                return Some(DiscoveredBrightness { source, devices });
            }
            Ok(_) => continue,
            Err(error) => {
                tracing::debug!(
                    backend = candidate.name,
                    %error,
                    "brightness discovery failed"
                );
            }
        }
    }

    None
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::brightness::model::{clamp_percent, BrightnessDeviceInfo, BrightnessReading};

    pub struct FakeBrightnessBackend {
        pub name: &'static str,
        pub infos: Vec<BrightnessDeviceInfo>,
        pub probe_result: Result<BrightnessCapabilities, BrightnessError>,
        pub values: Mutex<HashMap<BrightnessDeviceId, f64>>,
        pub writes: Mutex<Vec<(BrightnessDeviceId, f64)>>,
        pub reads: Mutex<usize>,
        pub failing: Mutex<HashSet<BrightnessDeviceId>>,
    }

    impl FakeBrightnessBackend {
        pub fn new(name: &'static str, infos: Vec<BrightnessDeviceInfo>) -> Self {
            Self {
                name,
                infos,
                probe_result: Ok(BrightnessCapabilities::CONTROL),
                values: Mutex::new(HashMap::new()),
                writes: Mutex::new(Vec::new()),
                reads: Mutex::new(0),
                failing: Mutex::new(HashSet::new()),
            }
        }

        pub async fn set_value(&self, device: &BrightnessDeviceId, percent: f64) {
            self.values
                .lock()
                .await
                .insert(device.clone(), clamp_percent(percent));
        }

        pub async fn writes(&self) -> Vec<(BrightnessDeviceId, f64)> {
            self.writes.lock().await.clone()
        }

        pub async fn read_count(&self) -> usize {
            *self.reads.lock().await
        }

        pub fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl BrightnessBackend for FakeBrightnessBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        fn supports(&self, device: &BrightnessDeviceId) -> bool {
            self.infos.iter().any(|info| &info.id == device)
        }

        async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError> {
            self.probe_result.clone()
        }

        async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError> {
            Ok(self.infos.clone())
        }

        async fn read(
            &self,
            device: &BrightnessDeviceId,
        ) -> Result<BrightnessReading, BrightnessError> {
            *self.reads.lock().await += 1;
            let values = self.values.lock().await;
            let percent = values.get(device).copied().unwrap_or(50.0);
            Ok(BrightnessReading {
                percent,
                raw: Some((percent * 10.0) as u32),
                max_raw: Some(1000),
            })
        }

        async fn write(
            &self,
            device: &BrightnessDeviceId,
            percent: f64,
        ) -> Result<(), BrightnessError> {
            self.writes
                .lock()
                .await
                .push((device.clone(), clamp_percent(percent)));
            if self.failing.lock().await.contains(device) {
                return Err(BrightnessError::Command("write failed".to_string()));
            }
            self.values
                .lock()
                .await
                .insert(device.clone(), clamp_percent(percent));
            Ok(())
        }
    }

    pub fn fake_candidate(backend: Arc<FakeBrightnessBackend>) -> BrightnessCandidate {
        BrightnessCandidate::new(backend.name, move || {
            Ok(backend.clone() as BrightnessBackendRef)
        })
    }
}
