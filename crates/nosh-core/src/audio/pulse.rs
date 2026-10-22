//! PulseAudio and pipewire-pulse backend.
//!
//! `libpulse` objects cannot cross threads, so one worker thread owns the main
//! loop and context. Async callers send commands and await the reply. The worker
//! subscribes to sink, source, and server changes and sends one `Changed`
//! notification. The service performs the re-read.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::Sender;
use async_trait::async_trait;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::introspect::{ServerInfo, SinkInfo, SourceInfo};
use pulse::context::subscribe::{Facility, InterestMaskSet};
use pulse::context::{Context, FlagSet as ContextFlagSet};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::operation::{Operation, State as OperationState};
use pulse::volume::{ChannelVolumes, Volume};

use crate::audio::backend::{AudioBackend, AudioBackendEvent, AudioCapabilities};
use crate::audio::model::{volume_icon, AudioEndpoint, AudioModel, EndpointKind};
use crate::error::BackendError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// The Pulse backend connects lazily on the first probe.
pub struct PulseBackend {
    connection: Mutex<Option<Arc<PulseConnection>>>,
}

impl PulseBackend {
    pub fn new() -> Self {
        Self {
            connection: Mutex::new(None),
        }
    }

    fn live_connection(&self) -> Option<Arc<PulseConnection>> {
        let mut guard = self.connection.lock().expect("pulse mutex poisoned");
        if let Some(connection) = guard.as_ref() {
            if connection.alive.load(Ordering::SeqCst) {
                return Some(connection.clone());
            }
        }
        *guard = None;
        None
    }

    async fn connection(&self) -> Result<Arc<PulseConnection>, BackendError> {
        if let Some(connection) = self.live_connection() {
            return Ok(connection);
        }

        let connection = tokio::task::spawn_blocking(PulseConnection::connect)
            .await
            .map_err(|error| {
                BackendError::Unavailable(format!("Pulse worker did not start: {error}"))
            })??;
        let connection = Arc::new(connection);
        *self.connection.lock().expect("pulse mutex poisoned") = Some(connection.clone());
        Ok(connection)
    }

    async fn request(
        &self,
        make: impl FnOnce(tokio::sync::oneshot::Sender<Result<(), String>>) -> PulseCommand,
    ) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let (reply, response) = tokio::sync::oneshot::channel();
        connection
            .commands
            .send(make(reply))
            .map_err(|_| BackendError::Unavailable("Pulse worker is gone".to_string()))?;

        match tokio::time::timeout(REQUEST_TIMEOUT, response).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => Err(BackendError::Request(message)),
            Ok(Err(_)) => Err(BackendError::Unavailable(
                "Pulse worker dropped the reply".to_string(),
            )),
            Err(_) => Err(BackendError::Unavailable(
                "Pulse worker did not answer in time".to_string(),
            )),
        }
    }
}

impl Default for PulseBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AudioBackend for PulseBackend {
    fn name(&self) -> &'static str {
        "pulse"
    }

    async fn probe(&self) -> Result<AudioCapabilities, BackendError> {
        let connection = self.connection().await?;
        if !connection.alive.load(Ordering::SeqCst) {
            return Err(BackendError::Unavailable(
                "Pulse connection is not alive".to_string(),
            ));
        }
        Ok(AudioCapabilities::READ | AudioCapabilities::WATCH | AudioCapabilities::WRITE)
    }

    async fn read(&self) -> Result<AudioModel, BackendError> {
        let connection = self.connection().await?;
        let (reply, response) = tokio::sync::oneshot::channel();
        connection
            .commands
            .send(PulseCommand::Read(reply))
            .map_err(|_| BackendError::Unavailable("Pulse worker is gone".to_string()))?;

        match tokio::time::timeout(REQUEST_TIMEOUT, response).await {
            Ok(Ok(Ok(model))) => Ok(model),
            Ok(Ok(Err(message))) => Err(BackendError::Request(message)),
            Ok(Err(_)) => Err(BackendError::Unavailable(
                "Pulse worker dropped the reply".to_string(),
            )),
            Err(_) => Err(BackendError::Unavailable(
                "Pulse worker did not answer in time".to_string(),
            )),
        }
    }

    async fn watch(&self, events: Sender<AudioBackendEvent>) -> Result<(), BackendError> {
        let connection = self.connection().await?;
        let changes = connection
            .changes
            .lock()
            .expect("pulse mutex poisoned")
            .take()
            .ok_or_else(|| {
                BackendError::Unavailable("the Pulse change stream is already in use".to_string())
            })?;

        while changes.recv().await.is_ok() {
            if events.send(AudioBackendEvent::Changed).await.is_err() {
                return Ok(());
            }
        }

        Err(BackendError::Unavailable(
            "the Pulse connection ended".to_string(),
        ))
    }

    async fn set_volume(
        &self,
        kind: EndpointKind,
        id: u32,
        volume: f64,
    ) -> Result<(), BackendError> {
        let volume = volume.clamp(0.0, kind.max_volume());
        self.request(|reply| PulseCommand::SetVolume {
            kind,
            id,
            volume,
            reply,
        })
        .await
    }

    async fn set_muted(
        &self,
        kind: EndpointKind,
        id: u32,
        muted: bool,
    ) -> Result<(), BackendError> {
        self.request(|reply| PulseCommand::SetMuted {
            kind,
            id,
            muted,
            reply,
        })
        .await
    }

    async fn set_default(&self, kind: EndpointKind, id: u32) -> Result<(), BackendError> {
        self.request(|reply| PulseCommand::SetDefault { kind, id, reply })
            .await
    }
}

enum PulseCommand {
    Read(tokio::sync::oneshot::Sender<Result<AudioModel, String>>),
    SetVolume {
        kind: EndpointKind,
        id: u32,
        volume: f64,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetMuted {
        kind: EndpointKind,
        id: u32,
        muted: bool,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetDefault {
        kind: EndpointKind,
        id: u32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Shutdown,
}

struct PulseConnection {
    commands: tokio::sync::mpsc::UnboundedSender<PulseCommand>,
    changes: Mutex<Option<async_channel::Receiver<()>>>,
    alive: Arc<AtomicBool>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

impl PulseConnection {
    fn connect() -> Result<Self, BackendError> {
        let (commands, command_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (change_sender, changes) = async_channel::unbounded();
        let (ready_sender, ready) = std_mpsc::sync_channel(1);
        let alive = Arc::new(AtomicBool::new(false));
        let worker_alive = alive.clone();

        let thread = thread::Builder::new()
            .name("nosh-pulse".to_string())
            .spawn(move || {
                pulse_worker(command_receiver, change_sender, ready_sender, worker_alive);
            })
            .map_err(|error| {
                BackendError::Unavailable(format!("cannot start the Pulse worker: {error}"))
            })?;

        match ready.recv_timeout(CONNECT_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                commands,
                changes: Mutex::new(Some(changes)),
                alive,
                thread: Mutex::new(Some(thread)),
            }),
            Ok(Err(message)) => Err(BackendError::Unavailable(message)),
            Err(_) => Err(BackendError::Unavailable(
                "Pulse did not become ready in time".to_string(),
            )),
        }
    }
}

impl Drop for PulseConnection {
    fn drop(&mut self) {
        let _ = self.commands.send(PulseCommand::Shutdown);
        if let Some(thread) = self.thread.lock().expect("pulse mutex poisoned").take() {
            let _ = thread.join();
        }
    }
}

fn pulse_worker(
    mut commands: tokio::sync::mpsc::UnboundedReceiver<PulseCommand>,
    changes: async_channel::Sender<()>,
    ready: std_mpsc::SyncSender<Result<(), String>>,
    alive: Arc<AtomicBool>,
) {
    let Some(mut mainloop) = Mainloop::new() else {
        let _ = ready.send(Err("cannot create the Pulse main loop".to_string()));
        return;
    };
    let Some(mut context) = Context::new(&mainloop, "nosh") else {
        let _ = ready.send(Err("cannot create a Pulse context".to_string()));
        return;
    };

    if let Err(error) = context.connect(None, ContextFlagSet::NOFLAGS, None) {
        let _ = ready.send(Err(format!("cannot connect to Pulse: {error}")));
        return;
    }

    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Err(message) = iterate(&mut mainloop, &context) {
            let _ = ready.send(Err(message));
            return;
        }
        match context.get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => {
                let _ = ready.send(Err("the Pulse connection failed".to_string()));
                return;
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            let _ = ready.send(Err("Pulse did not become ready in time".to_string()));
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }

    let change_sender = changes.clone();
    context.set_subscribe_callback(Some(Box::new(move |facility, _operation, _index| {
        if matches!(
            facility,
            Some(Facility::Sink) | Some(Facility::Source) | Some(Facility::Server)
        ) {
            let _ = change_sender.try_send(());
        }
    })));
    let _ = context.subscribe(
        InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
        |_| {},
    );

    alive.store(true, Ordering::SeqCst);
    if ready.send(Ok(())).is_err() {
        alive.store(false, Ordering::SeqCst);
        return;
    }

    let mut cache = PulseCache::default();

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                PulseCommand::Read(reply) => {
                    let _ = reply.send(read_model(&mut mainloop, &context, &mut cache));
                }
                PulseCommand::SetVolume {
                    kind,
                    id,
                    volume,
                    reply,
                } => {
                    let _ = reply.send(set_volume(
                        &mut mainloop,
                        &context,
                        &cache,
                        kind,
                        id,
                        volume,
                    ));
                }
                PulseCommand::SetMuted {
                    kind,
                    id,
                    muted,
                    reply,
                } => {
                    let _ = reply.send(set_muted(&mut mainloop, &context, kind, id, muted));
                }
                PulseCommand::SetDefault { kind, id, reply } => {
                    let _ = reply.send(set_default(&mut mainloop, &mut context, &cache, kind, id));
                }
                PulseCommand::Shutdown => {
                    alive.store(false, Ordering::SeqCst);
                    return;
                }
            }
        }

        if commands.is_closed() {
            alive.store(false, Ordering::SeqCst);
            return;
        }

        if let Err(message) = iterate(&mut mainloop, &context) {
            tracing::debug!(%message, "the Pulse connection ended");
            break;
        }
        match context.get_state() {
            pulse::context::State::Failed | pulse::context::State::Terminated => break,
            _ => {}
        }

        thread::sleep(Duration::from_millis(5));
    }

    alive.store(false, Ordering::SeqCst);
}

fn iterate(mainloop: &mut Mainloop, context: &Context) -> Result<(), String> {
    match mainloop.iterate(false) {
        IterateResult::Success(_) => {}
        IterateResult::Quit(_) => return Err("the Pulse main loop quit".to_string()),
        IterateResult::Err(error) => return Err(format!("the Pulse main loop failed: {error}")),
    }

    match context.get_state() {
        pulse::context::State::Failed | pulse::context::State::Terminated => {
            Err("the Pulse connection failed".to_string())
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Default)]
struct PulseCache {
    names: HashMap<(EndpointKind, u32), String>,
    channels: HashMap<(EndpointKind, u32), u8>,
}

#[derive(Debug, Default)]
struct PartialModel {
    sinks: Vec<(RawEndpoint, u8)>,
    sources: Vec<(RawEndpoint, u8)>,
    default_sink: Option<String>,
    default_source: Option<String>,
    failed: bool,
}

#[derive(Debug)]
struct RawEndpoint {
    id: u32,
    name: String,
    description: String,
    volume: f64,
    muted: bool,
}

fn read_model(
    mainloop: &mut Mainloop,
    context: &Context,
    cache: &mut PulseCache,
) -> Result<AudioModel, String> {
    let partial = Arc::new(Mutex::new(PartialModel::default()));
    let mut sink_operation;
    let mut source_operation;
    let mut server_operation;

    {
        let introspector = context.introspect();

        sink_operation = introspector.get_sink_info_list({
            let partial = partial.clone();
            move |result: ListResult<&SinkInfo>| match result {
                ListResult::Item(info) => {
                    partial
                        .lock()
                        .expect("pulse mutex poisoned")
                        .sinks
                        .push(raw_from_sink(info));
                }
                ListResult::End => {}
                ListResult::Error => {
                    partial.lock().expect("pulse mutex poisoned").failed = true;
                }
            }
        });

        source_operation = introspector.get_source_info_list({
            let partial = partial.clone();
            move |result: ListResult<&SourceInfo>| match result {
                ListResult::Item(info) => {
                    if info.monitor_of_sink.is_none() {
                        partial
                            .lock()
                            .expect("pulse mutex poisoned")
                            .sources
                            .push(raw_from_source(info));
                    }
                }
                ListResult::End => {}
                ListResult::Error => {
                    partial.lock().expect("pulse mutex poisoned").failed = true;
                }
            }
        });

        server_operation = introspector.get_server_info({
            let partial = partial.clone();
            move |info: &ServerInfo| {
                let mut partial = partial.lock().expect("pulse mutex poisoned");
                partial.default_sink = info.default_sink_name.as_deref().map(str::to_string);
                partial.default_source = info.default_source_name.as_deref().map(str::to_string);
            }
        });
    }

    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        let running = is_running(&sink_operation)
            || is_running(&source_operation)
            || is_running(&server_operation);
        if !running {
            break;
        }
        iterate(mainloop, context)?;
        if Instant::now() >= deadline {
            sink_operation.cancel();
            source_operation.cancel();
            server_operation.cancel();
            return Err("Pulse device read timed out".to_string());
        }
        thread::sleep(Duration::from_millis(1));
    }

    let partial = {
        let mut partial = partial.lock().expect("pulse mutex poisoned");
        std::mem::take(&mut *partial)
    };
    if partial.failed {
        return Err("Pulse returned an error while reading the devices".to_string());
    }

    cache.names.clear();
    cache.channels.clear();
    for (endpoint, channels) in &partial.sinks {
        cache
            .names
            .insert((EndpointKind::Sink, endpoint.id), endpoint.name.clone());
        cache
            .channels
            .insert((EndpointKind::Sink, endpoint.id), *channels);
    }
    for (endpoint, channels) in &partial.sources {
        cache
            .names
            .insert((EndpointKind::Source, endpoint.id), endpoint.name.clone());
        cache
            .channels
            .insert((EndpointKind::Source, endpoint.id), *channels);
    }

    let (sinks, default_sink) = assemble(
        EndpointKind::Sink,
        partial.sinks,
        partial.default_sink.as_deref(),
    );
    let (sources, default_source) = assemble(
        EndpointKind::Source,
        partial.sources,
        partial.default_source.as_deref(),
    );

    Ok(AudioModel {
        default_sink,
        default_source,
        sinks,
        sources,
    })
}

fn raw_from_sink(info: &SinkInfo) -> (RawEndpoint, u8) {
    (
        RawEndpoint {
            id: info.index,
            name: info.name.as_deref().unwrap_or_default().to_string(),
            description: info.description.as_deref().unwrap_or_default().to_string(),
            volume: normalized_volume(info.volume.avg()),
            muted: info.mute,
        },
        info.volume.len().max(1),
    )
}

fn raw_from_source(info: &SourceInfo) -> (RawEndpoint, u8) {
    (
        RawEndpoint {
            id: info.index,
            name: info.name.as_deref().unwrap_or_default().to_string(),
            description: info.description.as_deref().unwrap_or_default().to_string(),
            volume: normalized_volume(info.volume.avg()),
            muted: info.mute,
        },
        info.volume.len().max(1),
    )
}

/// `wpctl` and AstalWp report the Pulse software volume divided by the normal
/// volume, so `0.5` means 50 percent. The linear amplitude conversion would show
/// 12.5 percent instead.
fn normalized_volume(volume: Volume) -> f64 {
    volume.0 as f64 / Volume::NORMAL.0 as f64
}

fn assemble(
    kind: EndpointKind,
    raw: Vec<(RawEndpoint, u8)>,
    default_name: Option<&str>,
) -> (Vec<AudioEndpoint>, Option<AudioEndpoint>) {
    let mut endpoints: Vec<AudioEndpoint> = raw
        .into_iter()
        .map(|(raw, _)| {
            let name = if raw.name.is_empty() {
                format!("{}-{}", kind.label(), raw.id)
            } else {
                raw.name
            };
            let description = if raw.description.is_empty() {
                name.clone()
            } else {
                raw.description
            };
            let volume = AudioModel::clamped_volume(kind, raw.volume);
            AudioEndpoint {
                id: raw.id,
                name,
                description,
                volume,
                muted: raw.muted,
                icon_name: volume_icon(kind, volume, raw.muted).to_string(),
                is_default: false,
            }
        })
        .collect();

    endpoints.sort_by_key(|endpoint| endpoint.id);
    for endpoint in &mut endpoints {
        endpoint.is_default = default_name == Some(endpoint.name.as_str());
    }
    let default = endpoints
        .iter()
        .find(|endpoint| endpoint.is_default)
        .cloned();

    (endpoints, default)
}

fn is_running<C: ?Sized>(operation: &Operation<C>) -> bool {
    operation.get_state() == OperationState::Running
}

fn set_volume(
    mainloop: &mut Mainloop,
    context: &Context,
    cache: &PulseCache,
    kind: EndpointKind,
    id: u32,
    volume: f64,
) -> Result<(), String> {
    let channels = cache.channels.get(&(kind, id)).copied().unwrap_or(2).max(1);
    let mut volume_set = ChannelVolumes::default();
    volume_set.set(
        channels,
        Volume(normalized_to_volume(volume.clamp(0.0, kind.max_volume()))),
    );

    let outcome = Arc::new(Mutex::new(None));
    let mut introspector = context.introspect();
    let mut operation = match kind {
        EndpointKind::Sink => {
            let outcome = outcome.clone();
            introspector.set_sink_volume_by_index(
                id,
                &volume_set,
                Some(Box::new(move |ok| {
                    *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
                })),
            )
        }
        EndpointKind::Source => {
            let outcome = outcome.clone();
            introspector.set_source_volume_by_index(
                id,
                &volume_set,
                Some(Box::new(move |ok| {
                    *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
                })),
            )
        }
    };

    finish(mainloop, context, &mut operation, outcome)
}

fn set_muted(
    mainloop: &mut Mainloop,
    context: &Context,
    kind: EndpointKind,
    id: u32,
    muted: bool,
) -> Result<(), String> {
    let outcome = Arc::new(Mutex::new(None));
    let mut introspector = context.introspect();
    let mut operation = match kind {
        EndpointKind::Sink => {
            let outcome = outcome.clone();
            introspector.set_sink_mute_by_index(
                id,
                muted,
                Some(Box::new(move |ok| {
                    *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
                })),
            )
        }
        EndpointKind::Source => {
            let outcome = outcome.clone();
            introspector.set_source_mute_by_index(
                id,
                muted,
                Some(Box::new(move |ok| {
                    *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
                })),
            )
        }
    };

    finish(mainloop, context, &mut operation, outcome)
}

fn set_default(
    mainloop: &mut Mainloop,
    context: &mut Context,
    cache: &PulseCache,
    kind: EndpointKind,
    id: u32,
) -> Result<(), String> {
    let name = cache
        .names
        .get(&(kind, id))
        .cloned()
        .ok_or_else(|| format!("Pulse does not know the {} with id {id}", kind.label()))?;

    let outcome = Arc::new(Mutex::new(None));
    let mut operation = match kind {
        EndpointKind::Sink => {
            let outcome = outcome.clone();
            context.set_default_sink(&name, move |ok| {
                *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
            })
        }
        EndpointKind::Source => {
            let outcome = outcome.clone();
            context.set_default_source(&name, move |ok| {
                *outcome.lock().expect("pulse mutex poisoned") = Some(ok);
            })
        }
    };

    finish(mainloop, context, &mut operation, outcome)
}

fn finish<C: ?Sized>(
    mainloop: &mut Mainloop,
    context: &Context,
    operation: &mut Operation<C>,
    outcome: Arc<Mutex<Option<bool>>>,
) -> Result<(), String> {
    let deadline = Instant::now() + WRITE_TIMEOUT;
    loop {
        match operation.get_state() {
            OperationState::Running => {}
            OperationState::Done => break,
            OperationState::Cancelled => return Err("Pulse cancelled the write".to_string()),
        }

        if Instant::now() >= deadline {
            operation.cancel();
            return Err("Pulse write timed out".to_string());
        }
        iterate(mainloop, context)?;
        thread::sleep(Duration::from_millis(1));
    }

    match *outcome.lock().expect("pulse mutex poisoned") {
        Some(true) | None => Ok(()),
        Some(false) => Err("Pulse rejected the write".to_string()),
    }
}

fn normalized_to_volume(volume: f64) -> u32 {
    let raw = (volume * Volume::NORMAL.0 as f64).round();
    raw.clamp(0.0, Volume::MAX.0 as f64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: u32, name: &str, volume: f64, muted: bool) -> (RawEndpoint, u8) {
        (
            RawEndpoint {
                id,
                name: name.to_string(),
                description: format!("{name} description"),
                volume,
                muted,
            },
            2,
        )
    }

    #[test]
    fn assemble_marks_the_named_default_only() {
        let (endpoints, default) = assemble(
            EndpointKind::Sink,
            vec![raw(2, "b", 0.5, false), raw(1, "a", 0.2, false)],
            Some("b"),
        );

        assert_eq!(endpoints[0].id, 1);
        assert!(!endpoints[0].is_default);
        assert_eq!(default.map(|endpoint| endpoint.id), Some(2));
    }

    #[test]
    fn assemble_clamps_amplified_input() {
        let (endpoints, _) = assemble(
            EndpointKind::Source,
            vec![raw(3, "microphone", 1.8, false)],
            Some("microphone"),
        );

        assert_eq!(endpoints[0].volume, 1.0);
    }

    #[test]
    fn normalized_volume_matches_wpctl_percentages() {
        assert_eq!(normalized_volume(Volume::NORMAL), 1.0);
        assert_eq!(
            normalized_volume(Volume((Volume::NORMAL.0 as f64 * 1.5) as u32)),
            1.5
        );
        assert_eq!(
            normalized_to_volume(0.5),
            (Volume::NORMAL.0 as f64 * 0.5).round() as u32
        );
    }
}
