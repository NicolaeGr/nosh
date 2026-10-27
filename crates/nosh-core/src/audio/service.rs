use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;

use crate::audio::backend::{
    resolve_audio, AudioBackend, AudioBackendEvent, AudioBackendRef, AudioCandidate,
    AudioCapabilities, ResolvedAudio,
};
use crate::audio::model::{AudioModel, EndpointKind, VOLUME_EPSILON};
#[cfg(feature = "audio-pulse")]
use crate::audio::pulse::PulseBackend;
use crate::audio::wpctl::WpctlBackend;
use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::error::BackendError;
use crate::event::{AudioEvent, Event, Service, Snapshot};
use crate::origin::ChangeOrigin;

/// A backend that reports no audio devices. It keeps the shell running
/// without an audio server.
pub struct StaticAudioBackend;

#[async_trait]
impl AudioBackend for StaticAudioBackend {
    fn name(&self) -> &'static str {
        "static"
    }

    async fn probe(&self) -> Result<AudioCapabilities, BackendError> {
        Ok(AudioCapabilities::OBSERVE)
    }

    async fn read(&self) -> Result<AudioModel, BackendError> {
        Ok(AudioModel::default())
    }

    async fn watch(&self, _events: Sender<AudioBackendEvent>) -> Result<(), BackendError> {
        std::future::pending::<Result<(), BackendError>>().await
    }

    async fn set_volume(
        &self,
        _kind: EndpointKind,
        _id: u32,
        _volume: f64,
    ) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "set-volume",
        })
    }

    async fn set_muted(
        &self,
        _kind: EndpointKind,
        _id: u32,
        _muted: bool,
    ) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "set-mute",
        })
    }

    async fn set_default(&self, _kind: EndpointKind, _id: u32) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "set-default",
        })
    }
}

#[derive(Debug, Clone)]
pub struct AudioServiceConfig {
    pub required: AudioCapabilities,
    /// Merges slider movement into one backend write.
    pub debounce: Duration,
    pub missing_env_retry: Duration,
    pub retry: RetryPolicy,
    pub stable_connection: Duration,
    pub recovery_retry: RetryPolicy,
    pub recovery_stability: Duration,
    pub recovery_probe_interval: Duration,
}

impl Default for AudioServiceConfig {
    fn default() -> Self {
        Self {
            required: AudioCapabilities::OBSERVE,
            debounce: Duration::from_millis(35),
            missing_env_retry: Duration::from_secs(2),
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
            recovery_retry: RetryPolicy {
                initial: Duration::from_secs(1),
                max: Duration::from_secs(60),
                jitter: true,
            },
            recovery_stability: Duration::from_secs(5),
            recovery_probe_interval: Duration::from_secs(1),
        }
    }
}

pub struct AudioService {
    candidates: Vec<AudioCandidate>,
    config: AudioServiceConfig,
}

impl AudioService {
    pub fn from_env() -> Self {
        Self::from_env_with(AudioServiceConfig::default())
    }

    pub fn from_env_with(config: AudioServiceConfig) -> Self {
        let mut candidates = Vec::new();

        #[cfg(feature = "audio-pulse")]
        candidates.push(AudioCandidate::new("pulse", || {
            Ok(Arc::new(PulseBackend::new()) as AudioBackendRef)
        }));

        candidates.push(AudioCandidate::new("wpctl", || {
            Ok(Arc::new(WpctlBackend::new()) as AudioBackendRef)
        }));
        candidates.push(AudioCandidate::new("static", || {
            Ok(Arc::new(StaticAudioBackend) as AudioBackendRef)
        }));

        Self::with_candidates(candidates, config)
    }

    pub fn with_candidates(candidates: Vec<AudioCandidate>, config: AudioServiceConfig) -> Self {
        Self { candidates, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());
        let mut last_backend: Option<&'static str> = None;

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let resolved =
                match resolve_audio("audio", self.candidates.clone(), self.config.required).await {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        tracing::warn!(%error, "no audio backend is available");
                        if events
                            .send(Event::ServiceAvailability(Service::Audio, false))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        if wait_or_closed(&commands, self.config.missing_env_retry).await {
                            return;
                        }
                        continue;
                    }
                };

            if last_backend != Some(resolved.name) {
                last_backend = Some(resolved.name);
                if events
                    .send(Event::Audio(AudioEvent::BackendChanged {
                        backend: resolved.name,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            // The static backend reports no devices and accepts no writes. The
            // shell still starts and hides its audio widgets.
            let unavailable = resolved.name == "static";
            if unavailable
                && events
                    .send(Event::Audio(AudioEvent::Unavailable))
                    .await
                    .is_err()
            {
                return;
            }
            if events
                .send(Event::ServiceAvailability(Service::Audio, !unavailable))
                .await
                .is_err()
            {
                return;
            }

            let initial = match resolved.backend.read().await {
                Ok(model) => model,
                Err(error) => {
                    tracing::warn!(
                        backend = resolved.name,
                        %error,
                        "initial audio read failed"
                    );
                    if events
                        .send(Event::ServiceAvailability(Service::Audio, false))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if wait_or_closed(&commands, self.config.missing_env_retry).await {
                        return;
                    }
                    continue;
                }
            };

            let mut state = AudioState {
                model: initial.clone(),
                backend: resolved.name,
            };
            if emit_snapshot(&events, &state.model, resolved.name)
                .await
                .is_err()
            {
                return;
            }

            let (watch_tx, watch_rx) = async_channel::bounded(32);
            let backend = resolved.backend.clone();
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();

            let (recovery_tx, recovery_rx) = async_channel::bounded(1);
            let mut recovery_enabled = resolved.priority > 0;
            let mut recovery_task = if recovery_enabled {
                let candidates = self.candidates[..resolved.priority].to_vec();
                let config = self.config.clone();
                Some(tokio::spawn(async move {
                    recovery_loop(candidates, config, recovery_tx).await;
                }))
            } else {
                drop(recovery_tx);
                None
            };
            let mut recovered_backend = false;
            let mut pending: Option<PendingWrite> = None;
            let mut deadline: Option<tokio::time::Instant> = None;

            loop {
                let debounce = async {
                    match deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                };

                tokio::select! {
                    command = commands.recv() => {
                        match command {
                            Ok(command) => {
                                if let Some(plan) = plan_volume(&command, &state) {
                                    pending = Some(plan);
                                    deadline =
                                        Some(tokio::time::Instant::now() + self.config.debounce);
                                } else {
                                    // A non-volume command must wait for a
                                    // pending volume write.
                                    if let Some(write) = pending.take() {
                                        deadline = None;
                                        apply_pending(
                                            &resolved.backend,
                                            write,
                                            &mut state,
                                            &events,
                                        )
                                        .await;
                                    }
                                    if handle_command(
                                        &resolved.backend,
                                        command,
                                        &mut state,
                                        &events,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Err(_) => {
                                watch.abort();
                                return;
                            }
                        }
                    }
                    _ = debounce, if pending.is_some() => {
                        deadline = None;
                        if let Some(write) = pending.take() {
                            apply_pending(&resolved.backend, write, &mut state, &events).await;
                        }
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(AudioBackendEvent::Changed) => {
                                if refresh(
                                    &resolved.backend,
                                    &mut state,
                                    &events,
                                    None,
                                )
                                .await
                                .is_err()
                                {
                                    watch.abort();
                                    return;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    recovered = recovery_rx.recv(), if recovery_enabled => {
                        match recovered {
                            Ok(()) => {
                                tracing::info!(
                                    backend = resolved.name,
                                    "a more specific audio backend recovered"
                                );
                                recovered_backend = true;
                                break;
                            }
                            Err(_) => {
                                recovery_enabled = false;
                            }
                        }
                    }
                    result = &mut watch => {
                        match result {
                            Ok(Ok(())) => {
                                tracing::debug!(
                                    backend = resolved.name,
                                    "audio watch ended"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "audio watch failed"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "audio watch task panicked"
                                );
                            }
                        }
                        break;
                    }
                }
            }

            watch.abort();
            if let Some(task) = recovery_task.take() {
                task.abort();
            }

            if recovered_backend {
                backoff.reset();
                continue;
            }

            if events
                .send(Event::Audio(AudioEvent::Unavailable))
                .await
                .is_err()
            {
                return;
            }
            if events
                .send(Event::ServiceAvailability(Service::Audio, false))
                .await
                .is_err()
            {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the audio backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

struct AudioState {
    model: AudioModel,
    backend: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct PendingWrite {
    kind: EndpointKind,
    id: u32,
    volume: f64,
}

#[derive(Debug, Clone, Copy)]
enum UserTarget {
    Volume { kind: EndpointKind, id: u32 },
    Mute { kind: EndpointKind, id: u32 },
    Default,
}

/// Turns a volume command into a debounced write. Returns `None` for every
/// other command.
fn plan_volume(command: &Command, state: &AudioState) -> Option<PendingWrite> {
    match command {
        Command::AudioSetVolume(volume) => {
            let endpoint = state.model.default_for(EndpointKind::Sink)?;
            Some(PendingWrite {
                kind: EndpointKind::Sink,
                id: endpoint.id,
                volume: AudioModel::clamped_volume(EndpointKind::Sink, *volume),
            })
        }
        Command::AudioStepVolume(delta) => {
            let endpoint = state.model.default_for(EndpointKind::Sink)?;
            Some(PendingWrite {
                kind: EndpointKind::Sink,
                id: endpoint.id,
                volume: AudioModel::clamped_volume(EndpointKind::Sink, endpoint.volume + *delta),
            })
        }
        Command::AudioSetSourceVolume(volume) => {
            let endpoint = state.model.default_for(EndpointKind::Source)?;
            Some(PendingWrite {
                kind: EndpointKind::Source,
                id: endpoint.id,
                volume: AudioModel::clamped_volume(EndpointKind::Source, *volume),
            })
        }
        Command::AudioSetEndpointVolume { kind, id, volume } => Some(PendingWrite {
            kind: *kind,
            id: *id,
            volume: AudioModel::clamped_volume(*kind, *volume),
        }),
        _ => None,
    }
}

/// Applies everything except volume. Returns `Err` when the event channel
/// closed and the service should stop.
async fn handle_command(
    backend: &AudioBackendRef,
    command: Command,
    state: &mut AudioState,
    events: &Sender<Event>,
) -> Result<(), ()> {
    match command {
        Command::AudioSetMuted(muted) => {
            let Some(endpoint) = state.model.default_for(EndpointKind::Sink).cloned() else {
                return fail(events, "no default sink is available").await;
            };
            let target = UserTarget::Mute {
                kind: EndpointKind::Sink,
                id: endpoint.id,
            };
            write_and_refresh(
                backend,
                backend.set_muted(EndpointKind::Sink, endpoint.id, muted),
                state,
                events,
                target,
            )
            .await
        }
        Command::AudioToggleMute(kind) => {
            let Some(endpoint) = state.model.default_for(kind).cloned() else {
                return fail(events, &format!("no default {} is available", kind.label())).await;
            };
            let target = UserTarget::Mute {
                kind,
                id: endpoint.id,
            };
            write_and_refresh(
                backend,
                backend.set_muted(kind, endpoint.id, !endpoint.muted),
                state,
                events,
                target,
            )
            .await
        }
        Command::AudioSetEndpointMuted { kind, id, muted } => {
            if state.model.endpoint(kind, id).is_none() {
                return fail(events, &format!("audio endpoint {id} does not exist")).await;
            }
            let target = UserTarget::Mute { kind, id };
            write_and_refresh(
                backend,
                backend.set_muted(kind, id, muted),
                state,
                events,
                target,
            )
            .await
        }
        Command::AudioSetDefault(id) => {
            let kind = if state.model.endpoint(EndpointKind::Sink, id).is_some() {
                EndpointKind::Sink
            } else if state.model.endpoint(EndpointKind::Source, id).is_some() {
                EndpointKind::Source
            } else {
                return fail(events, &format!("audio endpoint {id} does not exist")).await;
            };

            write_and_refresh(
                backend,
                backend.set_default(kind, id),
                state,
                events,
                UserTarget::Default,
            )
            .await
        }
        Command::AudioSetVolume(_)
        | Command::AudioStepVolume(_)
        | Command::AudioSetSourceVolume(_)
        | Command::AudioSetEndpointVolume { .. } => {
            // These commands go through `plan_volume`. Report the missing
            // default endpoint instead of failing silently.
            fail(events, "no default audio endpoint is available").await
        }
        _ => Ok(()),
    }
}

async fn write_and_refresh(
    backend: &AudioBackendRef,
    write: impl std::future::Future<Output = Result<(), BackendError>>,
    state: &mut AudioState,
    events: &Sender<Event>,
    target: UserTarget,
) -> Result<(), ()> {
    match write.await {
        Ok(()) => refresh(backend, state, events, Some(target)).await,
        Err(error) => {
            tracing::warn!(%error, "audio write failed");
            fail(events, &error.to_string()).await?;
            refresh(backend, state, events, None).await
        }
    }
}

async fn apply_pending(
    backend: &AudioBackendRef,
    write: PendingWrite,
    state: &mut AudioState,
    events: &Sender<Event>,
) {
    let target = UserTarget::Volume {
        kind: write.kind,
        id: write.id,
    };
    let _ = write_and_refresh(
        backend,
        backend.set_volume(write.kind, write.id, write.volume),
        state,
        events,
        target,
    )
    .await;
}

/// Re-reads the model, publishes it, and emits change events with the right
/// origin. Returns `Err` when the event channel closed.
async fn refresh(
    backend: &AudioBackendRef,
    state: &mut AudioState,
    events: &Sender<Event>,
    user: Option<UserTarget>,
) -> Result<(), ()> {
    let next = match backend.read().await {
        Ok(model) => model,
        Err(error) => {
            tracing::warn!(%error, "audio read failed");
            let _ = events
                .send(Event::Audio(AudioEvent::ReadFailed(error.to_string())))
                .await;
            return Ok(());
        }
    };

    let previous = std::mem::replace(&mut state.model, next.clone());
    emit_snapshot(events, &next, state.backend).await?;
    emit_changes(&previous, &next, user, events).await
}

async fn emit_snapshot(
    events: &Sender<Event>,
    model: &AudioModel,
    backend: &'static str,
) -> Result<(), ()> {
    if events
        .send(Event::Audio(AudioEvent::Snapshot(model.clone())))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        audio: Some(model.clone()),
        audio_backend: Some(backend),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn emit_changes(
    previous: &AudioModel,
    next: &AudioModel,
    user: Option<UserTarget>,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let mut reported_volume = false;
    let mut reported_mute = false;

    for kind in [EndpointKind::Sink, EndpointKind::Source] {
        for endpoint in next.endpoints(kind) {
            let Some(old) = previous.endpoint(kind, endpoint.id) else {
                continue;
            };

            if (old.volume - endpoint.volume).abs() >= VOLUME_EPSILON {
                let origin = volume_origin(user, kind, endpoint.id);
                if origin == ChangeOrigin::User {
                    reported_volume = true;
                }
                if events
                    .send(Event::Audio(AudioEvent::VolumeChanged {
                        kind,
                        id: endpoint.id,
                        volume: endpoint.volume,
                        old: old.volume,
                        origin,
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }

            if old.muted != endpoint.muted {
                let origin = mute_origin(user, kind, endpoint.id);
                if origin == ChangeOrigin::User {
                    reported_mute = true;
                }
                if events
                    .send(Event::Audio(AudioEvent::MuteChanged {
                        kind,
                        id: endpoint.id,
                        muted: endpoint.muted,
                        origin,
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }
        }
    }

    // The server may accept a write without changing the value, for example
    // when the user steps past the maximum. The UI still needs feedback, so
    // report a user change.
    if let Some(UserTarget::Volume { kind, id }) = user {
        if !reported_volume {
            if let Some(endpoint) = next.endpoint(kind, id) {
                let old = previous
                    .endpoint(kind, id)
                    .map(|endpoint| endpoint.volume)
                    .unwrap_or(endpoint.volume);
                if events
                    .send(Event::Audio(AudioEvent::VolumeChanged {
                        kind,
                        id,
                        volume: endpoint.volume,
                        old,
                        origin: ChangeOrigin::User,
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }
        }
    }
    if let Some(UserTarget::Mute { kind, id }) = user {
        if !reported_mute {
            if let Some(endpoint) = next.endpoint(kind, id) {
                if events
                    .send(Event::Audio(AudioEvent::MuteChanged {
                        kind,
                        id,
                        muted: endpoint.muted,
                        origin: ChangeOrigin::User,
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }
        }
    }

    for kind in [EndpointKind::Sink, EndpointKind::Source] {
        let old = previous.default_for(kind).map(|endpoint| endpoint.id);
        let new = next.default_for(kind);
        if old != new.map(|endpoint| endpoint.id) {
            if let Some(endpoint) = new {
                if events
                    .send(Event::Audio(AudioEvent::DefaultChanged {
                        kind,
                        endpoint: endpoint.clone(),
                    }))
                    .await
                    .is_err()
                {
                    return Err(());
                }
            }
        }
    }

    Ok(())
}

fn volume_origin(user: Option<UserTarget>, kind: EndpointKind, id: u32) -> ChangeOrigin {
    match user {
        Some(UserTarget::Volume {
            kind: target_kind,
            id: target_id,
        }) if target_kind == kind && target_id == id => ChangeOrigin::User,
        _ => ChangeOrigin::External,
    }
}

fn mute_origin(user: Option<UserTarget>, kind: EndpointKind, id: u32) -> ChangeOrigin {
    match user {
        Some(UserTarget::Mute {
            kind: target_kind,
            id: target_id,
        }) if target_kind == kind && target_id == id => ChangeOrigin::User,
        _ => ChangeOrigin::External,
    }
}

async fn fail(events: &Sender<Event>, message: &str) -> Result<(), ()> {
    events
        .send(Event::Audio(AudioEvent::WriteFailed(message.to_string())))
        .await
        .map_err(|_| ())
}

async fn recovery_loop(
    candidates: Vec<AudioCandidate>,
    config: AudioServiceConfig,
    signal: Sender<()>,
) {
    let mut backoff = Backoff::new(config.recovery_retry, seed());

    loop {
        let delay = backoff.next_delay();
        tracing::debug!(?delay, "re-probing more specific audio backends");
        tokio::time::sleep(delay).await;

        let resolved = match resolve_audio("audio", candidates.clone(), config.required).await {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::debug!(%error, "no more specific audio backend is available");
                continue;
            }
        };

        match stable_backend(
            &resolved,
            config.required,
            config.recovery_stability,
            config.recovery_probe_interval,
        )
        .await
        {
            Ok(()) => {
                tracing::info!(backend = resolved.name, "specific audio backend is stable");
                if signal.send(()).await.is_err() {
                    return;
                }
                return;
            }
            Err(error) => {
                tracing::debug!(
                    backend = resolved.name,
                    %error,
                    "specific audio backend did not stay healthy"
                );
            }
        }
    }
}

async fn stable_backend(
    resolved: &ResolvedAudio,
    required: AudioCapabilities,
    stability: Duration,
    probe_interval: Duration,
) -> Result<(), BackendError> {
    let started = Instant::now();
    let interval = probe_interval.max(Duration::from_millis(1));

    loop {
        let remaining = stability.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        tokio::time::sleep(interval.min(remaining)).await;
        if started.elapsed() >= stability {
            return Ok(());
        }

        let capabilities = resolved.backend.probe().await?;
        if !capabilities.contains(required) {
            return Err(BackendError::Unsupported {
                backend: resolved.name,
                capability: "required audio capabilities",
            });
        }
    }
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping an audio command while disconnected");
            }
            result.is_err()
        }
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
    use crate::audio::backend::test_support::{FakeAudioBackend, FakeWrite};
    use crate::audio::model::{AudioEndpoint, MAX_VOLUME, VOLUME_STEP};

    fn endpoint(id: u32, volume: f64) -> AudioEndpoint {
        AudioEndpoint {
            id,
            name: format!("endpoint-{id}"),
            description: format!("Endpoint {id}"),
            volume,
            muted: false,
            icon_name: "audio-volume-medium-symbolic".to_string(),
            is_default: id == 1,
        }
    }

    fn model(volume: f64) -> AudioModel {
        let sink = endpoint(1, volume);
        AudioModel {
            default_sink: Some(sink.clone()),
            default_source: Some(endpoint(2, 0.5)),
            sinks: vec![sink, endpoint(3, 0.2)],
            sources: vec![endpoint(2, 0.5)],
        }
    }

    fn candidate(backend: Arc<FakeAudioBackend>) -> AudioCandidate {
        AudioCandidate::new(backend.name, move || Ok(backend.clone() as AudioBackendRef))
    }

    fn config() -> AudioServiceConfig {
        AudioServiceConfig {
            debounce: Duration::from_millis(10),
            missing_env_retry: Duration::from_millis(10),
            retry: RetryPolicy {
                initial: Duration::from_millis(10),
                max: Duration::from_millis(30),
                jitter: false,
            },
            stable_connection: Duration::from_millis(10),
            recovery_retry: RetryPolicy {
                initial: Duration::from_millis(10),
                max: Duration::from_millis(20),
                jitter: false,
            },
            recovery_stability: Duration::from_millis(10),
            recovery_probe_interval: Duration::from_millis(2),
            ..AudioServiceConfig::default()
        }
    }

    async fn wait_for_audio<F>(events: &Receiver<Event>, mut test: F) -> Event
    where
        F: FnMut(&Event) -> bool,
    {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("timed out waiting for an audio event")
                .expect("the event channel closed");
            if test(&event) {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn the_first_snapshot_has_no_origin_event() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::Snapshot(_)))
        })
        .await;

        // The first snapshot must not look like a user change, or the indicator
        // would flash at every startup.
        loop {
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Ok(Event::Audio(AudioEvent::VolumeChanged { .. }))) => {
                    panic!("the initial snapshot emitted a volume change")
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) | Err(_) => break,
            }
        }

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_user_volume_change_reports_the_user_origin() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        command_tx.send(Command::AudioSetVolume(0.4)).await.unwrap();

        let event = wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::VolumeChanged { .. }))
        })
        .await;
        match event {
            Event::Audio(AudioEvent::VolumeChanged {
                kind,
                id,
                volume,
                old,
                origin,
            }) => {
                assert_eq!(kind, EndpointKind::Sink);
                assert_eq!(id, 1);
                assert_eq!(volume, 0.4);
                assert_eq!(old, 0.3);
                assert_eq!(origin, ChangeOrigin::User);
            }
            other => panic!("unexpected event {other:?}"),
        }

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_user_mute_toggle_flips_the_default_and_reports_the_user_origin() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        command_tx
            .send(Command::AudioToggleMute(EndpointKind::Sink))
            .await
            .unwrap();

        let event = wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::MuteChanged { .. }))
        })
        .await;
        match event {
            Event::Audio(AudioEvent::MuteChanged {
                kind,
                id,
                muted,
                origin,
            }) => {
                assert_eq!(kind, EndpointKind::Sink);
                assert_eq!(id, 1);
                assert!(muted);
                assert_eq!(origin, ChangeOrigin::User);
            }
            other => panic!("unexpected event {other:?}"),
        }
        assert_eq!(
            backend.writes().await,
            vec![FakeWrite::Muted {
                kind: EndpointKind::Sink,
                id: 1,
                muted: true,
            }]
        );

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn an_external_change_does_not_use_the_user_origin() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        backend.set_model(model(0.8)).await;
        backend.push_external().await;

        let event = wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::VolumeChanged { .. }))
        })
        .await;
        match event {
            Event::Audio(AudioEvent::VolumeChanged { volume, origin, .. }) => {
                assert_eq!(volume, 0.8);
                assert_eq!(origin, ChangeOrigin::External);
            }
            other => panic!("unexpected event {other:?}"),
        }

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn slider_movement_merges_into_one_write() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let mut config = config();
        config.debounce = Duration::from_millis(50);
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config);
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        for volume in [0.35, 0.4, 0.45] {
            command_tx
                .send(Command::AudioSetVolume(volume))
                .await
                .unwrap();
        }

        wait_for_audio(&event_rx, |event| {
            matches!(
                event,
                Event::Audio(AudioEvent::VolumeChanged {
                    origin: ChangeOrigin::User,
                    ..
                })
            )
        })
        .await;

        let writes = backend.writes().await;
        assert_eq!(
            writes,
            vec![FakeWrite::Volume {
                kind: EndpointKind::Sink,
                id: 1,
                volume: 0.45,
            }]
        );

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn an_endpoint_volume_command_writes_that_endpoint() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let mut config = config();
        config.debounce = Duration::from_millis(10);
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config);
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        command_tx
            .send(Command::AudioSetEndpointVolume {
                kind: EndpointKind::Sink,
                id: 3,
                volume: 0.7,
            })
            .await
            .unwrap();

        wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::VolumeChanged { .. }))
        })
        .await;

        assert_eq!(
            backend.writes().await,
            vec![FakeWrite::Volume {
                kind: EndpointKind::Sink,
                id: 3,
                volume: 0.7,
            }]
        );

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn an_endpoint_mute_command_writes_that_endpoint() {
        let backend = FakeAudioBackend::new("fake", model(0.3)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        command_tx
            .send(Command::AudioSetEndpointMuted {
                kind: EndpointKind::Sink,
                id: 3,
                muted: true,
            })
            .await
            .unwrap();

        wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::MuteChanged { .. }))
        })
        .await;

        assert_eq!(
            backend.writes().await,
            vec![FakeWrite::Muted {
                kind: EndpointKind::Sink,
                id: 3,
                muted: true,
            }]
        );

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn step_commands_clamp_to_the_output_maximum() {
        let backend = FakeAudioBackend::new("fake", model(1.45)).arc();
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let mut config = config();
        config.debounce = Duration::from_millis(10);
        let service = AudioService::with_candidates(vec![candidate(backend.clone())], config);
        let task = tokio::spawn(service.run(command_rx, event_tx));

        backend.wait_until_watching().await;
        command_tx
            .send(Command::AudioStepVolume(VOLUME_STEP))
            .await
            .unwrap();

        wait_for_audio(&event_rx, |event| {
            matches!(event, Event::Audio(AudioEvent::VolumeChanged { .. }))
        })
        .await;

        let writes = backend.writes().await;
        assert!(matches!(
            writes.as_slice(),
            [FakeWrite::Volume { volume, .. }] if *volume == MAX_VOLUME
        ));

        drop(command_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_static_backend_starts_and_reports_unavailable() {
        let backend = Arc::new(StaticAudioBackend);
        let candidate =
            AudioCandidate::new("static", move || Ok(backend.clone() as AudioBackendRef));
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = AudioService::with_candidates(vec![candidate], config());
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let event = wait_for_audio(&event_rx, |event| {
            matches!(event, Event::ServiceAvailability(Service::Audio, false))
        })
        .await;
        assert!(matches!(
            event,
            Event::ServiceAvailability(Service::Audio, false)
        ));

        drop(command_tx);
        task.await.unwrap();
    }
}
