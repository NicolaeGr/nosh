//! The MPRIS service.
//!
//! Publishes a snapshot at startup and re-reads the player after every change
//! or transport command. A missing session bus reports the feature as
//! unavailable and retries with backoff, so the rest of the shell starts
//! normally.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::event::{Event, MediaEvent, Service, Snapshot};
use crate::media::backend::MediaBackendRef;
use crate::media::model::MediaModel;
use crate::media::mpris::MprisBackend;

#[derive(Debug, Clone)]
pub struct MediaServiceConfig {
    pub retry: RetryPolicy,
    /// A connection that lasted this long resets the backoff.
    pub stable_connection: Duration,
    /// How often to re-read the player while connected. Players publish
    /// metadata in stages, and some fields, such as Firefox cover art, can
    /// arrive after the change signal without a second signal.
    pub poll: Duration,
}

impl Default for MediaServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
            poll: Duration::from_secs(1),
        }
    }
}

pub struct MediaService {
    backend: MediaBackendRef,
    config: MediaServiceConfig,
}

impl MediaService {
    pub fn from_env() -> Self {
        Self::from_env_with(MediaServiceConfig::default())
    }

    pub fn from_env_with(config: MediaServiceConfig) -> Self {
        Self::with_backend(Arc::new(MprisBackend::new()), config)
    }

    pub fn with_backend(backend: MediaBackendRef, config: MediaServiceConfig) -> Self {
        Self { backend, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            if let Err(error) = self.backend.probe().await {
                tracing::debug!(
                    backend = self.backend.name(),
                    %error,
                    "the media player bus is not available"
                );
                if report_unavailable(&events).await.is_err() {
                    return;
                }
                let delay = backoff.next_delay();
                if wait_or_closed(&commands, delay).await {
                    return;
                }
                continue;
            }

            if events
                .send(Event::ServiceAvailability(Service::Media, true))
                .await
                .is_err()
            {
                return;
            }

            let mut state = match self.backend.read().await {
                Ok(model) => model,
                Err(error) => {
                    tracing::warn!(
                        backend = self.backend.name(),
                        %error,
                        "the initial media read failed"
                    );
                    if report_unavailable(&events).await.is_err() {
                        return;
                    }
                    let delay = backoff.next_delay();
                    if wait_or_closed(&commands, delay).await {
                        return;
                    }
                    continue;
                }
            };
            if emit_snapshot(&events, &self.backend, &state).await.is_err() {
                return;
            }

            let (watch_tx, watch_rx) = async_channel::bounded(32);
            let backend = Arc::clone(&self.backend);
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();
            let mut poll = tokio::time::interval(self.config.poll);
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The initial read above already covered the first tick.
            poll.tick().await;

            loop {
                tokio::select! {
                    result = commands.recv() => {
                        let Ok(command) = result else {
                            watch.abort();
                            return;
                        };
                        if handle_command(&self.backend, command, &state, &events)
                            .await
                            .is_err()
                        {
                            watch.abort();
                            return;
                        }
                        if refresh(&self.backend, &mut state, &events).await.is_err() {
                            watch.abort();
                            return;
                        }
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(_) => {
                                if refresh(&self.backend, &mut state, &events).await.is_err() {
                                    watch.abort();
                                    return;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    _ = poll.tick() => {
                        if refresh(&self.backend, &mut state, &events).await.is_err() {
                            watch.abort();
                            return;
                        }
                    }
                    result = &mut watch => {
                        match result {
                            Ok(Ok(())) => {
                                tracing::debug!(
                                    backend = self.backend.name(),
                                    "the media watch ended"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(
                                    backend = self.backend.name(),
                                    %error,
                                    "the media watch failed"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    backend = self.backend.name(),
                                    %error,
                                    "the media watch task panicked"
                                );
                            }
                        }
                        break;
                    }
                }
            }

            watch.abort();
            if events
                .send(Event::Media(MediaEvent::Unavailable))
                .await
                .is_err()
            {
                return;
            }
            if events
                .send(Event::ServiceAvailability(Service::Media, false))
                .await
                .is_err()
            {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the media backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

async fn handle_command(
    backend: &MediaBackendRef,
    command: Command,
    state: &MediaModel,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let Some(player) = state.player.as_deref() else {
        return Ok(());
    };
    let result = match command {
        Command::MediaPlayPause => backend.play_pause(player).await,
        Command::MediaNext => backend.next(player).await,
        Command::MediaPrevious => backend.previous(player).await,
        Command::MediaSeek(millis) => backend.seek(player, Duration::from_millis(millis)).await,
        Command::MediaSetVolume(volume) => backend.set_volume(player, volume).await,
        _ => return Ok(()),
    };

    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(
                backend = backend.name(),
                %error,
                "a media command failed"
            );
            events
                .send(Event::Media(MediaEvent::WriteFailed(error.to_string())))
                .await
                .map_err(|_| ())
        }
    }
}

async fn refresh(
    backend: &MediaBackendRef,
    state: &mut MediaModel,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let next = match backend.read().await {
        Ok(model) => model,
        Err(error) => {
            tracing::warn!(
                backend = backend.name(),
                %error,
                "a media read after a change failed"
            );
            return Ok(());
        }
    };

    if next != *state {
        *state = next.clone();
        events
            .send(Event::Media(MediaEvent::Changed(next)))
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}

async fn emit_snapshot(
    events: &Sender<Event>,
    backend: &MediaBackendRef,
    state: &MediaModel,
) -> Result<(), ()> {
    events
        .send(Event::Snapshot(Snapshot {
            media: Some(state.clone()),
            media_backend: Some(backend.name()),
            ..Snapshot::default()
        }))
        .await
        .map_err(|_| ())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    events
        .send(Event::Media(MediaEvent::Unavailable))
        .await
        .map_err(|_| ())?;
    events
        .send(Event::ServiceAvailability(Service::Media, false))
        .await
        .map_err(|_| ())
}

async fn wait_or_closed(commands: &Receiver<Command>, delay: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(delay) => false,
        _ = commands.recv() => true,
    }
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::error::BackendError;
    use crate::event::MediaEvent;
    use crate::media::backend::test_support::{FakeMediaBackend, FakeWrite};
    use crate::media::model::{MediaModel, PlaybackStatus};

    fn track(title: &str) -> MediaModel {
        MediaModel {
            player: Some("org.mpris.MediaPlayer2.test".to_string()),
            identity: "Test".to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            length: Some(Duration::from_secs(200)),
            status: PlaybackStatus::Playing,
            can_next: true,
            ..MediaModel::default()
        }
    }

    fn service(backend: &Arc<FakeMediaBackend>) -> MediaService {
        MediaService::with_backend(
            Arc::clone(backend) as MediaBackendRef,
            MediaServiceConfig {
                retry: RetryPolicy {
                    initial: Duration::from_millis(10),
                    max: Duration::from_millis(20),
                    jitter: false,
                },
                stable_connection: Duration::from_millis(10),
                poll: Duration::from_secs(60),
            },
        )
    }

    fn start(service: MediaService) -> (Sender<Command>, async_channel::Receiver<Event>) {
        let (commands, command_receiver) = async_channel::unbounded();
        let (events, event_receiver) = async_channel::unbounded();
        tokio::spawn(service.run(command_receiver, events));
        (commands, event_receiver)
    }

    async fn next_media(events: &async_channel::Receiver<Event>) -> MediaEvent {
        loop {
            let event = events.recv().await.unwrap();
            if let Event::Media(event) = event {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_availability_and_a_snapshot() {
        let backend = FakeMediaBackend::new("fake", track("First")).arc();
        let (_commands, events) = start(service(&backend));

        loop {
            let Event::Snapshot(snapshot) = events.recv().await.unwrap() else {
                continue;
            };
            let Some(model) = snapshot.media else {
                continue;
            };
            assert_eq!(model.title, "First");
            assert_eq!(snapshot.media_backend, Some("fake"));
            return;
        }
    }

    #[tokio::test]
    async fn an_external_change_emits_the_new_model() {
        let backend = FakeMediaBackend::new("fake", track("First")).arc();
        let (_commands, events) = start(service(&backend));
        backend.wait_until_watching().await;

        backend.set_model(track("Second")).await;
        backend.push_external().await;

        assert_eq!(
            next_media(&events).await,
            MediaEvent::Changed(track("Second"))
        );
    }

    #[tokio::test]
    async fn transport_commands_reach_the_backend() {
        let backend = FakeMediaBackend::new("fake", track("First")).arc();
        let (commands, events) = start(service(&backend));
        backend.wait_until_watching().await;

        commands.send(Command::MediaNext).await.unwrap();
        commands.send(Command::MediaPrevious).await.unwrap();
        commands.send(Command::MediaSeek(90_000)).await.unwrap();
        commands.send(Command::MediaSetVolume(0.3)).await.unwrap();
        commands.send(Command::MediaPlayPause).await.unwrap();

        let expected = vec![
            FakeWrite::Next,
            FakeWrite::Previous,
            FakeWrite::Seek(Duration::from_secs(90)),
            FakeWrite::Volume(0.3),
            FakeWrite::PlayPause,
        ];
        let mut writes = Vec::new();
        for _ in 0..50 {
            writes = backend.writes().await;
            if writes == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(writes, expected);
        let _ = events;
    }

    #[tokio::test]
    async fn a_late_metadata_change_is_found_without_a_signal() {
        let backend = FakeMediaBackend::new("fake", track("First")).arc();
        let service = MediaService::with_backend(
            Arc::clone(&backend) as MediaBackendRef,
            MediaServiceConfig {
                retry: RetryPolicy {
                    initial: Duration::from_millis(10),
                    max: Duration::from_millis(20),
                    jitter: false,
                },
                stable_connection: Duration::from_millis(10),
                poll: Duration::from_millis(20),
            },
        );
        let (_commands, events) = start(service);
        backend.wait_until_watching().await;

        // A player may add a field such as the cover art URL without sending
        // another change signal. The poll still has to pick it up.
        backend.set_model(track("Second")).await;

        assert_eq!(
            next_media(&events).await,
            MediaEvent::Changed(track("Second"))
        );
    }

    #[tokio::test]
    async fn a_missing_backend_reports_unavailable_and_retries() {
        let mut fake = FakeMediaBackend::new("fake", track("First"));
        fake.probe_result = Err(BackendError::Unavailable("no bus".to_string()));
        let backend = fake.arc();
        let (_commands, events) = start(service(&backend));

        assert_eq!(next_media(&events).await, MediaEvent::Unavailable);
    }
}
