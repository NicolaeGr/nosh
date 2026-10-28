use std::sync::Arc;
use std::time::Duration;

use async_channel::Sender;
use async_trait::async_trait;

use crate::error::BackendError;
use crate::media::model::MediaModel;

/// A change reported by a running media backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaBackendEvent {
    /// The track, playback state, or controls changed.
    Changed,
}

/// One implementation of MPRIS reading and control.
///
/// A session bus without players is not an error. Reads return an empty model
/// and writes do nothing until a player appears.
#[async_trait]
pub trait MediaBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Checks that the bus is reachable. A missing player is not a failure.
    async fn probe(&self) -> Result<(), BackendError>;

    /// The player the shell should show, or an empty model when nothing runs.
    async fn read(&self) -> Result<MediaModel, BackendError>;

    /// Runs until the connection fails or the event receiver closes.
    async fn watch(&self, events: Sender<MediaBackendEvent>) -> Result<(), BackendError>;

    async fn play_pause(&self, player: &str) -> Result<(), BackendError>;

    async fn next(&self, player: &str) -> Result<(), BackendError>;

    async fn previous(&self, player: &str) -> Result<(), BackendError>;

    /// Seeks the track of `player` to an absolute position.
    async fn seek(&self, player: &str, position: Duration) -> Result<(), BackendError>;

    async fn set_volume(&self, player: &str, volume: f64) -> Result<(), BackendError>;
}

pub type MediaBackendRef = Arc<dyn MediaBackend>;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;
    use std::time::Duration;

    use async_channel::Sender;
    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    pub enum FakeWrite {
        PlayPause,
        Next,
        Previous,
        Seek(Duration),
        Volume(f64),
    }

    pub struct FakeMediaBackend {
        pub name: &'static str,
        pub probe_result: Result<(), BackendError>,
        pub model: Mutex<MediaModel>,
        pub writes: Mutex<Vec<FakeWrite>>,
        pub events: Mutex<Option<Sender<MediaBackendEvent>>>,
        pub watch_ready: Sender<()>,
        pub watch_ready_rx: async_channel::Receiver<()>,
    }

    impl FakeMediaBackend {
        pub fn new(name: &'static str, model: MediaModel) -> Self {
            let (watch_ready, watch_ready_rx) = async_channel::unbounded();
            Self {
                name,
                probe_result: Ok(()),
                model: Mutex::new(model),
                writes: Mutex::new(Vec::new()),
                events: Mutex::new(None),
                watch_ready,
                watch_ready_rx,
            }
        }

        pub async fn push_external(&self) {
            let sender = self.events.lock().await.clone();
            if let Some(sender) = sender {
                let _ = sender.send(MediaBackendEvent::Changed).await;
            }
        }

        pub async fn set_model(&self, model: MediaModel) {
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
    impl MediaBackend for FakeMediaBackend {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn probe(&self) -> Result<(), BackendError> {
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<MediaModel, BackendError> {
            Ok(self.model.lock().await.clone())
        }

        async fn watch(&self, events: Sender<MediaBackendEvent>) -> Result<(), BackendError> {
            *self.events.lock().await = Some(events);
            let _ = self.watch_ready.send(()).await;
            std::future::pending::<()>().await;
            Ok(())
        }

        async fn play_pause(&self, _player: &str) -> Result<(), BackendError> {
            self.writes.lock().await.push(FakeWrite::PlayPause);
            let mut model = self.model.lock().await;
            model.status = if model.status.is_playing() {
                crate::media::model::PlaybackStatus::Paused
            } else {
                crate::media::model::PlaybackStatus::Playing
            };
            Ok(())
        }

        async fn next(&self, _player: &str) -> Result<(), BackendError> {
            self.writes.lock().await.push(FakeWrite::Next);
            Ok(())
        }

        async fn previous(&self, _player: &str) -> Result<(), BackendError> {
            self.writes.lock().await.push(FakeWrite::Previous);
            Ok(())
        }

        async fn seek(&self, _player: &str, position: Duration) -> Result<(), BackendError> {
            self.writes.lock().await.push(FakeWrite::Seek(position));
            self.model.lock().await.position = position;
            Ok(())
        }

        async fn set_volume(&self, _player: &str, volume: f64) -> Result<(), BackendError> {
            self.writes.lock().await.push(FakeWrite::Volume(volume));
            self.model.lock().await.volume = volume;
            Ok(())
        }
    }
}
