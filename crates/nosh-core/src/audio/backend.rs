use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use bitflags::bitflags;

use crate::audio::model::{AudioModel, EndpointKind};
use crate::error::BackendError;

bitflags! {
    /// What an audio backend supports.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AudioCapabilities: u8 {
        const READ = 1 << 0;
        const WATCH = 1 << 1;
        const WRITE = 1 << 2;
    }
}

impl AudioCapabilities {
    /// Read and watch, the minimum needed to publish a model.
    pub const OBSERVE: Self = Self::READ.union(Self::WATCH);
}

/// A change reported by a running backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioBackendEvent {
    /// Sinks, sources, defaults, volume, or mute changed.
    Changed,
}

/// One implementation of audio reading, watching, and writing.
#[async_trait]
pub trait AudioBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Must not change volume, mute, or the default device.
    async fn probe(&self) -> Result<AudioCapabilities, BackendError>;

    async fn read(&self) -> Result<AudioModel, BackendError>;

    /// Runs until the connection fails or the event receiver closes.
    async fn watch(&self, events: Sender<AudioBackendEvent>) -> Result<(), BackendError>;

    async fn set_volume(
        &self,
        kind: EndpointKind,
        id: u32,
        volume: f64,
    ) -> Result<(), BackendError>;

    async fn set_muted(&self, kind: EndpointKind, id: u32, muted: bool)
        -> Result<(), BackendError>;

    async fn set_default(&self, kind: EndpointKind, id: u32) -> Result<(), BackendError>;
}

pub type AudioBackendRef = Arc<dyn AudioBackend>;

type BackendFactory = Arc<dyn Fn() -> Result<AudioBackendRef, BackendError> + Send + Sync>;

#[derive(Clone)]
pub struct AudioCandidate {
    pub name: &'static str,
    make: BackendFactory,
}

impl AudioCandidate {
    pub fn new<F>(name: &'static str, make: F) -> Self
    where
        F: Fn() -> Result<AudioBackendRef, BackendError> + Send + Sync + 'static,
    {
        Self {
            name,
            make: Arc::new(make),
        }
    }

    pub fn create(&self) -> Result<AudioBackendRef, BackendError> {
        (self.make)()
    }
}

pub struct ResolvedAudio {
    pub backend: AudioBackendRef,
    pub name: &'static str,
    pub capabilities: AudioCapabilities,
    /// Position in the candidate list. Zero is the most specific backend.
    pub priority: usize,
}

/// Tries audio candidates from most specific to most generic.
pub async fn resolve_audio(
    feature: &'static str,
    candidates: Vec<AudioCandidate>,
    required: AudioCapabilities,
) -> Result<ResolvedAudio, BackendError> {
    for (priority, candidate) in candidates.into_iter().enumerate() {
        let backend = match candidate.create() {
            Ok(backend) => backend,
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "audio backend constructor failed"
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
                    "selected audio backend"
                );
                return Ok(ResolvedAudio {
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
                    "audio backend lacks a required capability"
                );
            }
            Err(error) => {
                tracing::debug!(
                    feature,
                    backend = candidate.name,
                    %error,
                    "audio backend probe failed"
                );
            }
        }
    }

    tracing::debug!(
        feature,
        "no audio backend satisfies the required capabilities"
    );
    Err(BackendError::NoBackend)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_channel::Sender;
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::audio::model::{AudioEndpoint, MAX_VOLUME};

    #[derive(Debug, Clone, PartialEq)]
    pub enum FakeWrite {
        Volume {
            kind: EndpointKind,
            id: u32,
            volume: f64,
        },
        Muted {
            kind: EndpointKind,
            id: u32,
            muted: bool,
        },
        Default {
            kind: EndpointKind,
            id: u32,
        },
    }

    pub struct FakeAudioBackend {
        pub name: &'static str,
        pub probe_result: Result<AudioCapabilities, BackendError>,
        pub watch_result: Result<(), BackendError>,
        pub hold_watch: bool,
        pub write_result: Result<(), BackendError>,
        pub model: Mutex<AudioModel>,
        pub writes: Mutex<Vec<FakeWrite>>,
        pub watches: AtomicUsize,
        pub events: Mutex<Option<Sender<AudioBackendEvent>>>,
        pub watch_ready: Sender<()>,
        pub watch_ready_rx: async_channel::Receiver<()>,
    }

    impl FakeAudioBackend {
        pub fn new(name: &'static str, model: AudioModel) -> Self {
            let (watch_ready, watch_ready_rx) = async_channel::unbounded();
            Self {
                name,
                probe_result: Ok(AudioCapabilities::READ
                    | AudioCapabilities::WATCH
                    | AudioCapabilities::WRITE),
                watch_result: Ok(()),
                hold_watch: true,
                write_result: Ok(()),
                model: Mutex::new(model),
                writes: Mutex::new(Vec::new()),
                watches: AtomicUsize::new(0),
                events: Mutex::new(None),
                watch_ready,
                watch_ready_rx,
            }
        }

        pub async fn push_external(&self) {
            let sender = self.events.lock().await.clone();
            if let Some(sender) = sender {
                let _ = sender.send(AudioBackendEvent::Changed).await;
            }
        }

        pub async fn set_model(&self, model: AudioModel) {
            *self.model.lock().await = model;
        }

        pub async fn writes(&self) -> Vec<FakeWrite> {
            self.writes.lock().await.clone()
        }

        pub async fn wait_until_watching(&self) {
            let _ = tokio::time::timeout(Duration::from_secs(1), self.watch_ready_rx.recv()).await;
        }

        pub fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl AudioBackend for FakeAudioBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn probe(&self) -> Result<AudioCapabilities, BackendError> {
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<AudioModel, BackendError> {
            Ok(self.model.lock().await.clone())
        }

        async fn watch(&self, events: Sender<AudioBackendEvent>) -> Result<(), BackendError> {
            self.watches.fetch_add(1, Ordering::SeqCst);
            *self.events.lock().await = Some(events);
            let _ = self.watch_ready.send(()).await;
            if self.hold_watch {
                std::future::pending::<()>().await;
            }
            self.watch_result.clone()
        }

        async fn set_volume(
            &self,
            kind: EndpointKind,
            id: u32,
            volume: f64,
        ) -> Result<(), BackendError> {
            self.writes
                .lock()
                .await
                .push(FakeWrite::Volume { kind, id, volume });
            self.write_result.clone()?;

            let mut model = self.model.lock().await;
            let updated = {
                let endpoints = match kind {
                    EndpointKind::Sink => &mut model.sinks,
                    EndpointKind::Source => &mut model.sources,
                };
                endpoints
                    .iter_mut()
                    .find(|endpoint| endpoint.id == id)
                    .map(|endpoint| {
                        endpoint.volume = volume.clamp(0.0, MAX_VOLUME);
                        endpoint.clone()
                    })
            };
            if let Some(updated) = updated {
                let default = match kind {
                    EndpointKind::Sink => model.default_sink.as_mut(),
                    EndpointKind::Source => model.default_source.as_mut(),
                };
                if let Some(default) = default {
                    if default.id == id {
                        *default = updated;
                    }
                }
            }
            Ok(())
        }

        async fn set_muted(
            &self,
            kind: EndpointKind,
            id: u32,
            muted: bool,
        ) -> Result<(), BackendError> {
            self.writes
                .lock()
                .await
                .push(FakeWrite::Muted { kind, id, muted });
            self.write_result.clone()?;

            let mut model = self.model.lock().await;
            let endpoints = match kind {
                EndpointKind::Sink => &mut model.sinks,
                EndpointKind::Source => &mut model.sources,
            };
            if let Some(endpoint) = endpoints.iter_mut().find(|endpoint| endpoint.id == id) {
                endpoint.muted = muted;
            }
            Ok(())
        }

        async fn set_default(&self, kind: EndpointKind, id: u32) -> Result<(), BackendError> {
            self.writes
                .lock()
                .await
                .push(FakeWrite::Default { kind, id });
            self.write_result.clone()?;

            let mut model = self.model.lock().await;
            let endpoint = AudioEndpoint {
                is_default: true,
                ..model
                    .endpoint(kind, id)
                    .cloned()
                    .ok_or_else(|| BackendError::Request("no such endpoint".to_string()))?
            };
            match kind {
                EndpointKind::Sink => model.default_sink = Some(endpoint),
                EndpointKind::Source => model.default_source = Some(endpoint),
            }
            for endpoint in &mut model.sinks {
                endpoint.is_default = kind == EndpointKind::Sink && endpoint.id == id;
            }
            for endpoint in &mut model.sources {
                endpoint.is_default = kind == EndpointKind::Source && endpoint.id == id;
            }
            Ok(())
        }
    }
}
