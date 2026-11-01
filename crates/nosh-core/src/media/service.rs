//! The MPRIS service.
//!
//! Publishes a snapshot at startup and re-reads the player after every change
//! or transport command. A missing session bus reports the feature as
//! unavailable and retries with backoff, so the rest of the shell starts
//! normally.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::event::{Event, MediaEvent};
use crate::media::backend::MediaBackendRef;
use crate::media::model::MediaModel;
use crate::media::mpris::MprisBackend;
use crate::service::{seed, wait_or_closed};

/// Fast polls after a change, before the interval backs off.
const SETTLE_POLLS: u8 = 5;
/// The steady-state interval is this many times the configured poll.
const SLOW_POLL_FACTOR: u32 = 5;
/// A paused player is polled this rarely.
const PAUSED_POLL: Duration = Duration::from_secs(30);

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
            if emit_snapshot(&events, &state).await.is_err() {
                return;
            }

            let (watch_tx, watch_rx) = async_channel::bounded(32);
            let backend = Arc::clone(&self.backend);
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();
            // Players publish metadata in stages right after a change, so the
            // watch and command paths are followed by a burst of fast polls
            // before the interval backs off. Only those paths reset it: a
            // playing position changes on every read and would otherwise keep
            // the fast interval forever.
            let mut settle = SETTLE_POLLS;
            // The service polls only while a media view is on screen. When the
            // panel is closed the watch keeps the model current, and the
            // position is not shown anyway.
            let mut visible = false;

            loop {
                tokio::select! {
                    result = commands.recv() => {
                        let Ok(command) = result else {
                            watch.abort();
                            return;
                        };
                        let command = match command {
                            Command::MediaSetVisible(shown) => {
                                if shown != visible {
                                    visible = shown;
                                    settle = SETTLE_POLLS;
                                    if shown
                                        && refresh(&self.backend, &mut state, &events)
                                            .await
                                            .is_err()
                                    {
                                        watch.abort();
                                        return;
                                    }
                                }
                                continue;
                            }
                            command => command,
                        };
                        settle = SETTLE_POLLS;
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
                                settle = SETTLE_POLLS;
                                if refresh(&self.backend, &mut state, &events).await.is_err() {
                                    watch.abort();
                                    return;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    _ = poll_sleep(poll_delay(&state, self.config.poll, settle, visible)) => {
                        settle = settle.saturating_sub(1);
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

    if next == *state {
        return Ok(());
    }
    *state = next.clone();
    events
        .send(Event::Media(MediaEvent::Changed(next)))
        .await
        .map_err(|_| ())
}

async fn emit_snapshot(events: &Sender<Event>, state: &MediaModel) -> Result<(), ()> {
    events
        .send(Event::Media(MediaEvent::Snapshot(state.clone())))
        .await
        .map_err(|_| ())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    events
        .send(Event::Media(MediaEvent::Unavailable))
        .await
        .map_err(|_| ())
}

/// How often to re-read the player without a signal, or `None` to not poll.
/// The service polls only while a media view is on screen. A change is
/// followed by a burst of fast polls because players stage their metadata, and
/// the steady state is slow because the panel advances the position itself. A
/// paused player only needs an occasional safety net, and a player that is not
/// there needs nothing: the watch reports it appearing or changing.
fn poll_delay(state: &MediaModel, active: Duration, settle: u8, visible: bool) -> Option<Duration> {
    if !visible {
        return None;
    }
    if state.status.is_playing() {
        if settle > 0 {
            Some(active)
        } else {
            Some(active * SLOW_POLL_FACTOR)
        }
    } else if state.is_active() {
        Some(active.max(PAUSED_POLL))
    } else {
        None
    }
}

async fn poll_sleep(delay: Option<Duration>) {
    match delay {
        Some(delay) => tokio::time::sleep(delay).await,
        None => std::future::pending().await,
    }
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
            match events.recv().await.unwrap() {
                Event::Media(MediaEvent::Snapshot(_)) => continue,
                Event::Media(event) => return event,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_availability_and_a_snapshot() {
        let backend = FakeMediaBackend::new("fake", track("First")).arc();
        let (_commands, events) = start(service(&backend));

        let model = loop {
            match events.recv().await.unwrap() {
                Event::Media(MediaEvent::Snapshot(model)) => break model,
                _ => continue,
            }
        };
        assert_eq!(model.title, "First");
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
        let (commands, events) = start(service);
        backend.wait_until_watching().await;
        commands.send(Command::MediaSetVisible(true)).await.unwrap();

        // A player may add a field such as the cover art URL without sending
        // another change signal. The poll still has to pick it up.
        backend.set_model(track("Second")).await;

        assert_eq!(
            next_media(&events).await,
            MediaEvent::Changed(track("Second"))
        );
    }

    #[tokio::test]
    async fn a_hidden_media_view_does_not_poll() {
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

        // No media view is on screen, so the service must not read the player.
        // The watch reports changes on its own.
        backend.set_model(track("Second")).await;
        let quiet = tokio::time::timeout(Duration::from_millis(100), next_media(&events)).await;
        assert!(quiet.is_err(), "the hidden service polled the player");
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
