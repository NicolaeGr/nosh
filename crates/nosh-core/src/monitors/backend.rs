use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::monitors::model::{MonitorCapabilities, MonitorLayout};

/// A change produced by a running monitor backend.
#[derive(Debug, Clone)]
pub enum MonitorBackendEvent {
    /// A fresh read after a monitor event.
    Layout(MonitorLayout),
}

/// One implementation of monitor reading, watching, and writing.
///
/// Read and write capability are resolved independently by the service, so a
/// backend may implement only the read half.
#[async_trait]
pub trait MonitorBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Must not change state.
    async fn probe(&self) -> Result<MonitorCapabilities, BackendError>;

    async fn read(&self) -> Result<MonitorLayout, BackendError>;

    /// Runs until the connection fails or the event receiver closes.
    async fn watch(&self, events: Sender<MonitorBackendEvent>) -> Result<(), BackendError>;

    async fn apply(&self, layout: &MonitorLayout) -> Result<(), BackendError>;
}

pub type MonitorBackendRef = Arc<dyn MonitorBackend>;

type BackendFactory = Arc<dyn Fn() -> Result<MonitorBackendRef, BackendError> + Send + Sync>;

#[derive(Clone)]
pub struct MonitorCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl MonitorCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> Result<MonitorBackendRef, BackendError> + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> Result<MonitorBackendRef, BackendError> {
        (self.make)()
    }
}

pub struct ResolvedMonitor {
    pub backend: MonitorBackendRef,
    pub name: &'static str,
    pub capabilities: MonitorCapabilities,
    /// Position in the candidate list. Zero is the most specific backend.
    pub priority: usize,
}

/// Tries monitor candidates from most specific to most generic.
pub async fn resolve_monitor(
    feature: &'static str,
    candidates: Vec<MonitorCandidate>,
    required: MonitorCapabilities,
) -> Result<ResolvedMonitor, BackendError> {
    for (priority, candidate) in candidates.into_iter().enumerate() {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "monitor backend constructor failed"
                );
                continue;
            }
        };

        match backend.probe().await {
            Ok(capabilities) if capabilities.contains(required) => {
                tracing::info!(
                    feature,
                    backend = candidate.name,
                    ?capabilities,
                    "selected monitor backend"
                );
                return Ok(ResolvedMonitor {
                    backend,
                    name: candidate.name,
                    capabilities,
                    priority,
                });
            }
            Ok(capabilities) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    ?capabilities,
                    "monitor backend lacks a required capability"
                );
            }
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "monitor backend probe failed"
                );
            }
        }
    }

    tracing::debug!(
        feature,
        "no monitor backend satisfies the required capabilities"
    );
    Err(BackendError::NoBackend)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_channel::{Receiver, Sender};
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;

    pub struct FakeMonitorBackend {
        pub name: &'static str,
        pub probe_result: Result<MonitorCapabilities, BackendError>,
        pub read_result: Mutex<Result<MonitorLayout, BackendError>>,
        pub watch_result: Result<(), BackendError>,
        pub watch_delay: Option<std::time::Duration>,
        pub watches: AtomicUsize,
        pub probe_failures: AtomicUsize,
        pub apply_result: Mutex<Result<(), BackendError>>,
        pub applied: Mutex<Vec<MonitorLayout>>,
        pub watch_events: Mutex<Option<Receiver<MonitorLayout>>>,
    }

    impl FakeMonitorBackend {
        pub fn new(name: &'static str, capabilities: MonitorCapabilities) -> Self {
            Self {
                name,
                probe_result: Ok(capabilities),
                read_result: Mutex::new(Ok(MonitorLayout::default())),
                watch_result: Err(BackendError::Unavailable("watch ended".to_string())),
                watch_delay: None,
                watches: AtomicUsize::new(0),
                probe_failures: AtomicUsize::new(0),
                apply_result: Mutex::new(Ok(())),
                applied: Mutex::new(Vec::new()),
                watch_events: Mutex::new(None),
            }
        }

        pub fn watch_count(&self) -> usize {
            self.watches.load(Ordering::SeqCst)
        }

        pub async fn applied_layouts(&self) -> Vec<MonitorLayout> {
            self.applied.lock().await.clone()
        }

        pub fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl MonitorBackend for FakeMonitorBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn probe(&self) -> Result<MonitorCapabilities, BackendError> {
            if self.probe_failures.load(Ordering::SeqCst) > 0 {
                self.probe_failures.fetch_sub(1, Ordering::SeqCst);
                return Err(BackendError::Unavailable("probe failed".to_string()));
            }
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<MonitorLayout, BackendError> {
            self.read_result.lock().await.clone()
        }

        async fn watch(&self, events: Sender<MonitorBackendEvent>) -> Result<(), BackendError> {
            self.watches.fetch_add(1, Ordering::SeqCst);

            if let Some(receiver) = self.watch_events.lock().await.take() {
                while let Ok(layout) = receiver.recv().await {
                    if events
                        .send(MonitorBackendEvent::Layout(layout))
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            } else if let Some(delay) = self.watch_delay {
                tokio::time::sleep(delay).await;
            }

            self.watch_result.clone()
        }

        async fn apply(&self, layout: &MonitorLayout) -> Result<(), BackendError> {
            self.applied.lock().await.push(layout.clone());
            self.apply_result.lock().await.clone()
        }
    }
}
