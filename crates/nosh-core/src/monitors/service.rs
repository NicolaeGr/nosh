use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::error::{BackendError, MonitorError};
use crate::event::{Event, MonitorEvent, Service, Snapshot};
use crate::hyprland::{Hyprland, HyprlandMonitorBackend};
use crate::monitors::backend::{
    resolve_monitor, MonitorBackend, MonitorBackendEvent, MonitorBackendRef, MonitorCandidate,
    ResolvedMonitor,
};
use crate::monitors::model::{MonitorCapabilities, MonitorLayout};
use crate::monitors::preset::{preset_layout, resolve_target, LayoutPreset};

/// A backend that reports no monitors. It keeps the shell running on a
/// compositor without a monitor adapter.
pub struct StaticMonitorBackend;

#[async_trait]
impl MonitorBackend for StaticMonitorBackend {
    fn name(&self) -> &'static str {
        "static"
    }

    async fn probe(&self) -> Result<MonitorCapabilities, BackendError> {
        Ok(MonitorCapabilities::OBSERVE)
    }

    async fn read(&self) -> Result<MonitorLayout, BackendError> {
        Ok(MonitorLayout::default())
    }

    async fn watch(&self, _events: Sender<MonitorBackendEvent>) -> Result<(), BackendError> {
        std::future::pending::<Result<(), BackendError>>().await
    }

    async fn apply(&self, _layout: &MonitorLayout) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "apply",
        })
    }
}

#[derive(Debug, Clone)]
pub struct MonitorServiceConfig {
    pub required: MonitorCapabilities,
    pub missing_env_retry: Duration,
    pub retry: RetryPolicy,
    pub stable_connection: Duration,
    pub recovery_retry: RetryPolicy,
    pub recovery_stability: Duration,
    pub recovery_probe_interval: Duration,
}

impl Default for MonitorServiceConfig {
    fn default() -> Self {
        Self {
            required: MonitorCapabilities::OBSERVE,
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

pub struct MonitorService {
    read_candidates: Vec<MonitorCandidate>,
    write_candidates: Vec<MonitorCandidate>,
    config: MonitorServiceConfig,
}

impl MonitorService {
    pub fn from_env() -> Self {
        Self::from_env_with(MonitorServiceConfig::default())
    }

    pub fn from_env_with(config: MonitorServiceConfig) -> Self {
        let connection = Hyprland::detect().unwrap_or_else(|error| {
            tracing::debug!(%error, "cannot resolve the Hyprland connection");
            None
        });

        let read_hyprland = MonitorCandidate::new("hyprland", {
            let connection = connection.clone();
            move || match &connection {
                Some(connection) => Ok(Arc::new(HyprlandMonitorBackend::with_connection(
                    Arc::clone(connection),
                )) as MonitorBackendRef),
                None => Err(BackendError::Unavailable(
                    "HYPRLAND_INSTANCE_SIGNATURE is not set".to_string(),
                )),
            }
        });
        let read_static = MonitorCandidate::new("static", || {
            Ok(Arc::new(StaticMonitorBackend) as MonitorBackendRef)
        });

        let write_hyprland = MonitorCandidate::new("hyprland", {
            let connection = connection.clone();
            move || match &connection {
                Some(connection) => Ok(Arc::new(HyprlandMonitorBackend::with_connection(
                    Arc::clone(connection),
                )) as MonitorBackendRef),
                None => Err(BackendError::Unavailable(
                    "HYPRLAND_INSTANCE_SIGNATURE is not set".to_string(),
                )),
            }
        });

        Self::with_candidates(
            vec![read_hyprland, read_static],
            vec![write_hyprland],
            config,
        )
    }

    pub fn with_candidates(
        read_candidates: Vec<MonitorCandidate>,
        write_candidates: Vec<MonitorCandidate>,
        config: MonitorServiceConfig,
    ) -> Self {
        Self {
            read_candidates,
            write_candidates,
            config,
        }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());
        let mut last_backend: Option<&'static str> = None;
        let mut state = MonitorState::default();

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let resolved = match resolve_monitor(
                "monitors-read",
                self.read_candidates.clone(),
                self.config.required,
            )
            .await
            {
                Ok(resolved) => resolved,
                Err(error) => {
                    tracing::warn!(%error, "no monitor read backend is available");
                    if events
                        .send(Event::ServiceAvailability(Service::Monitors, false))
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

            let write = resolve_monitor(
                "monitors-write",
                self.write_candidates.clone(),
                MonitorCapabilities::WRITE,
            )
            .await;
            let write = match write {
                Ok(write) => Some(write),
                Err(error) => {
                    tracing::debug!(%error, "monitor writes are unavailable; running read-only");
                    None
                }
            };

            if last_backend != Some(resolved.name) {
                last_backend = Some(resolved.name);
                if events
                    .send(Event::Monitors(MonitorEvent::BackendChanged {
                        backend: resolved.name,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            // The static fallback reports no monitors. Mark the service
            // unavailable but keep the recovery loop probing for Hyprland.
            let fallback_only = resolved.name == "static";
            if fallback_only && emit_unavailable(&events).await.is_err() {
                return;
            }
            if events
                .send(Event::ServiceAvailability(
                    Service::Monitors,
                    !fallback_only,
                ))
                .await
                .is_err()
            {
                return;
            }

            let initial = match resolved.backend.read().await {
                Ok(layout) => layout,
                Err(error) => {
                    tracing::warn!(
                        backend = resolved.name,
                        %error,
                        "initial monitor read failed"
                    );
                    if emit_unavailable(&events).await.is_err() {
                        return;
                    }
                    if events
                        .send(Event::ServiceAvailability(Service::Monitors, false))
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

            let layout = state.adopt(initial);
            if events
                .send(Event::Monitors(MonitorEvent::Snapshot {
                    layout: layout.clone(),
                    backend: resolved.name,
                }))
                .await
                .is_err()
            {
                return;
            }
            let snapshot = Snapshot {
                monitors: Some(layout),
                monitor_backend: Some(resolved.name),
                ..Snapshot::default()
            };
            if events.send(Event::Snapshot(snapshot)).await.is_err() {
                return;
            }

            let (watch_tx, watch_rx) = async_channel::bounded(32);
            let backend = resolved.backend.clone();
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();

            let (recovery_tx, recovery_rx) = async_channel::bounded(1);
            let mut recovery_enabled = resolved.priority > 0;
            let mut recovery_task = if recovery_enabled {
                let candidates = self.read_candidates[..resolved.priority].to_vec();
                let config = self.config.clone();
                Some(tokio::spawn(async move {
                    recovery_loop(candidates, config, recovery_tx).await;
                }))
            } else {
                drop(recovery_tx);
                None
            };
            let mut recovered_backend = false;

            loop {
                tokio::select! {
                    command = commands.recv() => {
                        match command {
                            Ok(command) => {
                                handle_command(
                                    command,
                                    write.as_ref(),
                                    &mut state,
                                    &events,
                                )
                                .await;
                            }
                            Err(_) => {
                                watch.abort();
                                return;
                            }
                        }
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(MonitorBackendEvent::Layout(layout)) => {
                                let layout = state.adopt(layout);
                                if events
                                    .send(Event::Monitors(MonitorEvent::LayoutChanged(layout)))
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
                                    "a more specific monitor backend recovered"
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
                                    "monitor watch ended"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "monitor watch failed"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "monitor watch task panicked"
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

            if emit_unavailable(&events).await.is_err() {
                return;
            }
            if events
                .send(Event::ServiceAvailability(Service::Monitors, false))
                .await
                .is_err()
            {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the monitor backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

#[derive(Debug, Default)]
struct MonitorState {
    layout: Option<MonitorLayout>,
    primary: Option<String>,
    modes: HashMap<String, LayoutPreset>,
}

impl MonitorState {
    /// Takes a fresh read and keeps the previous primary when it is still
    /// enabled.
    fn adopt(&mut self, mut layout: MonitorLayout) -> MonitorLayout {
        self.primary = layout.apply_primary(self.primary.as_deref());
        self.layout = Some(layout.clone());
        layout
    }

    fn current(&self) -> Result<MonitorLayout, MonitorError> {
        self.layout.clone().ok_or(MonitorError::NoLayout)
    }

    fn set_primary(&mut self, connector: &str) -> Result<MonitorLayout, MonitorError> {
        let Some(layout) = self.layout.as_mut() else {
            return Err(MonitorError::NoLayout);
        };
        let exists = layout
            .monitors
            .iter()
            .any(|monitor| monitor.enabled && monitor.connector() == connector);
        if !exists {
            return Err(MonitorError::TargetNotFound(connector.to_string()));
        }

        layout.set_primary(Some(connector.to_string()));
        self.primary = Some(connector.to_string());
        Ok(layout.clone())
    }
}

async fn handle_command(
    command: Command,
    write: Option<&ResolvedMonitor>,
    state: &mut MonitorState,
    events: &Sender<Event>,
) {
    match command {
        Command::MonitorSetPrimary(connector) => match state.set_primary(&connector) {
            Ok(layout) => {
                let _ = events
                    .send(Event::Monitors(MonitorEvent::LayoutChanged(layout)))
                    .await;
            }
            Err(error) => fail(events, error).await,
        },
        Command::MonitorApply(mut layout) => {
            let _ = layout.respect_primary(state.primary.as_deref());
            write_and_publish(write, layout, state, events).await;
        }
        Command::MonitorSetEnabled { connector, enabled } => {
            let result = set_enabled(state, &connector, enabled);
            match result {
                Ok(layout) => {
                    write_and_publish(write, layout, state, events).await;
                }
                Err(error) => fail(events, error).await,
            }
        }
        Command::MonitorMode { target, mode } => {
            apply_mode(write, state, events, &target, mode).await;
        }
        Command::MonitorCycle { target, mode } => {
            let preset = match mode {
                Some(mode) => Some(mode),
                None => match state.current() {
                    Ok(layout) => match resolve_target(&layout, &target) {
                        Ok(monitor) => {
                            let connector = monitor.connector().to_string();
                            Some(
                                state
                                    .modes
                                    .get(&connector)
                                    .copied()
                                    .unwrap_or(LayoutPreset::Extend)
                                    .next(),
                            )
                        }
                        Err(error) => {
                            fail(events, error).await;
                            None
                        }
                    },
                    Err(error) => {
                        fail(events, error).await;
                        None
                    }
                },
            };

            if let Some(preset) = preset {
                apply_mode(write, state, events, &target, preset).await;
            }
        }
        _ => {}
    }
}

async fn apply_mode(
    write: Option<&ResolvedMonitor>,
    state: &mut MonitorState,
    events: &Sender<Event>,
    target: &str,
    preset: LayoutPreset,
) {
    let current = match state.current() {
        Ok(layout) => layout,
        Err(error) => {
            fail(events, error).await;
            return;
        }
    };
    let connector = match resolve_target(&current, target) {
        Ok(monitor) => monitor.connector().to_string(),
        Err(error) => {
            fail(events, error).await;
            return;
        }
    };
    let layout = match preset_layout(&current, preset, Some(&connector)) {
        Ok(layout) => layout,
        Err(error) => {
            fail(events, error).await;
            return;
        }
    };

    if write_and_publish(write, layout, state, events).await {
        state.modes.insert(connector, preset);
    }
}

fn set_enabled(
    state: &MonitorState,
    connector: &str,
    enabled: bool,
) -> Result<MonitorLayout, MonitorError> {
    let mut layout = state.current()?;
    let monitor = layout
        .monitor(connector)
        .ok_or_else(|| MonitorError::TargetNotFound(connector.to_string()))?;
    if enabled && (monitor.width <= 0 || monitor.height <= 0) {
        return Err(MonitorError::MissingResolution(connector.to_string()));
    }

    if let Some(monitor) = layout.monitor_mut(connector) {
        monitor.enabled = enabled;
        if !enabled {
            monitor.mirror_of = None;
        }
    }
    let _ = layout.respect_primary(state.primary.as_deref());
    Ok(layout)
}

/// Sends a layout to the write backend and publishes it only after the
/// compositor accepts every request.
async fn write_and_publish(
    write: Option<&ResolvedMonitor>,
    layout: MonitorLayout,
    state: &mut MonitorState,
    events: &Sender<Event>,
) -> bool {
    let Some(write) = write else {
        fail(events, MonitorError::ReadOnly).await;
        return false;
    };

    match write.backend.apply(&layout).await {
        Ok(()) => {
            state.primary = layout.primary.clone();
            state.layout = Some(layout.clone());
            events
                .send(Event::Monitors(MonitorEvent::LayoutChanged(layout)))
                .await
                .is_ok()
        }
        Err(error) => {
            fail(events, MonitorError::Write(error.to_string())).await;
            false
        }
    }
}

async fn fail(events: &Sender<Event>, error: MonitorError) {
    tracing::warn!(%error, "monitor command failed");
    let _ = events
        .send(Event::Monitors(MonitorEvent::ApplyFailed(
            error.to_string(),
        )))
        .await;
}

/// Re-probes the read candidates ahead of the selected backend. A candidate must
/// pass probes for the full stability window before the service switches back to
/// it.
async fn recovery_loop(
    candidates: Vec<MonitorCandidate>,
    config: MonitorServiceConfig,
    signal: Sender<()>,
) {
    let mut backoff = Backoff::new(config.recovery_retry, seed());

    loop {
        let delay = backoff.next_delay();
        tracing::debug!(?delay, "re-probing more specific monitor backends");
        tokio::time::sleep(delay).await;

        let resolved =
            match resolve_monitor("monitors-read", candidates.clone(), config.required).await {
                Ok(resolved) => resolved,
                Err(error) => {
                    tracing::debug!(%error, "no more specific monitor backend is available");
                    continue;
                }
            };

        tracing::debug!(
            backend = resolved.name,
            "specific monitor backend probed healthy"
        );
        match stable_backend(
            &resolved,
            config.required,
            config.recovery_stability,
            config.recovery_probe_interval,
        )
        .await
        {
            Ok(()) => {
                tracing::info!(
                    backend = resolved.name,
                    "specific monitor backend is stable"
                );
                if signal.send(()).await.is_err() {
                    return;
                }
                return;
            }
            Err(error) => {
                tracing::debug!(
                    backend = resolved.name,
                    %error,
                    "specific monitor backend did not stay healthy"
                );
            }
        }
    }
}

async fn stable_backend(
    resolved: &ResolvedMonitor,
    required: MonitorCapabilities,
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
                capability: "required monitor capabilities",
            });
        }
    }
}

async fn emit_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    events
        .send(Event::Monitors(MonitorEvent::Unavailable))
        .await
        .map(|_| ())
        .map_err(|_| ())
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a monitor command while disconnected");
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
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::*;
    use crate::monitors::backend::test_support::FakeMonitorBackend;
    use crate::monitors::model::{MonitorId, MonitorModel, Transform};

    fn candidate(name: &'static str, backend: Arc<FakeMonitorBackend>) -> MonitorCandidate {
        MonitorCandidate::new(name, move || Ok(backend.clone()))
    }

    fn fast_config() -> MonitorServiceConfig {
        MonitorServiceConfig {
            required: MonitorCapabilities::OBSERVE,
            missing_env_retry: Duration::from_millis(5),
            retry: RetryPolicy {
                initial: Duration::from_millis(5),
                max: Duration::from_millis(20),
                jitter: false,
            },
            stable_connection: Duration::from_secs(5),
            recovery_retry: RetryPolicy {
                initial: Duration::from_millis(5),
                max: Duration::from_millis(20),
                jitter: false,
            },
            recovery_stability: Duration::from_millis(20),
            recovery_probe_interval: Duration::from_millis(5),
        }
    }

    fn monitor(connector: &str, id: i32, focused: bool) -> MonitorModel {
        MonitorModel {
            id: MonitorId {
                compositor_id: id,
                connector: connector.to_string(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
            },
            enabled: true,
            focused,
            primary: false,
            x: id * 1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            transform: Transform::Normal,
            refresh_rate: 60.0,
            mirror_of: None,
        }
    }

    fn layout(monitors: Vec<MonitorModel>) -> MonitorLayout {
        let mut layout = MonitorLayout {
            monitors,
            primary: None,
        };
        let _ = layout.apply_primary(None);
        layout
    }

    async fn wait_for_snapshot(events: &Receiver<Event>) -> MonitorLayout {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(Event::Snapshot(snapshot)) = events.recv().await {
                    if let Some(layout) = snapshot.monitors {
                        break layout;
                    }
                }
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn the_static_backend_keeps_the_service_running_without_a_compositor() {
        let service = MonitorService::with_candidates(
            vec![
                MonitorCandidate::new("broken", || {
                    Err(BackendError::Unavailable("missing".into()))
                }),
                candidate(
                    "static",
                    FakeMonitorBackend::new("static", MonitorCapabilities::OBSERVE).arc(),
                ),
            ],
            vec![],
            fast_config(),
        );
        let (_command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();

        let task = tokio::spawn(service.run(command_rx, event_tx));
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::BackendChanged { backend: "static" })
        ));

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, Event::Monitors(MonitorEvent::Unavailable)));

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::ServiceAvailability(Service::Monitors, false)
        ));

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::Snapshot { .. })
        ));

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, Event::Snapshot(_)));

        task.abort();
    }

    #[tokio::test]
    async fn a_failed_watch_emits_unavailable_and_reconnects() {
        let mut backend = FakeMonitorBackend::new("fake", MonitorCapabilities::OBSERVE);
        backend.watch_result = Err(BackendError::Unavailable("closed".into()));
        let backend = backend.arc();
        let service = MonitorService::with_candidates(
            vec![candidate("fake", backend.clone())],
            vec![],
            fast_config(),
        );
        let (_command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();

        let task = tokio::spawn(service.run(command_rx, event_tx));

        let mut saw_available = false;
        let mut saw_unavailable = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);

        while tokio::time::Instant::now() < deadline && backend.watch_count() < 2 {
            match tokio::time::timeout(Duration::from_millis(50), event_rx.recv()).await {
                Ok(Ok(Event::ServiceAvailability(_, true))) => saw_available = true,
                Ok(Ok(Event::ServiceAvailability(_, false))) => saw_unavailable = true,
                _ => {}
            }
        }

        assert!(saw_available, "expected an availability true event");
        assert!(saw_unavailable, "expected an availability false event");
        assert!(backend.watch_count() >= 2, "expected a reconnect");

        task.abort();
    }

    #[tokio::test]
    async fn a_write_failure_is_reported_as_apply_failed() {
        let mut read = FakeMonitorBackend::new("read", MonitorCapabilities::OBSERVE);
        *read.read_result.lock().await = Ok(layout(vec![monitor("DP-1", 0, true)]));
        read.watch_delay = Some(Duration::from_secs(5));
        read.watch_result = Ok(());
        let read = read.arc();

        let write = FakeMonitorBackend::new(
            "write",
            MonitorCapabilities::WRITE | MonitorCapabilities::READ,
        );
        *write.apply_result.lock().await = Err(BackendError::Request("rejected".to_string()));
        let write = write.arc();

        let service = MonitorService::with_candidates(
            vec![candidate("read", read)],
            vec![candidate("write", write)],
            fast_config(),
        );
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        wait_for_snapshot(&event_rx).await;
        command_tx
            .send(Command::MonitorSetEnabled {
                connector: "DP-1".to_string(),
                enabled: false,
            })
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::ApplyFailed(message)) if message.contains("rejected")
        ));

        task.abort();
    }

    #[tokio::test]
    async fn a_missing_target_reports_an_error_and_keeps_the_service_running() {
        let mut read = FakeMonitorBackend::new("read", MonitorCapabilities::OBSERVE);
        *read.read_result.lock().await = Ok(layout(vec![
            monitor("DP-1", 0, true),
            monitor("DP-2", 1, false),
        ]));
        read.watch_delay = Some(Duration::from_secs(5));
        read.watch_result = Ok(());
        let read = read.arc();

        let service =
            MonitorService::with_candidates(vec![candidate("read", read)], vec![], fast_config());
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        wait_for_snapshot(&event_rx).await;
        command_tx
            .send(Command::MonitorSetPrimary("MISSING".to_string()))
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::ApplyFailed(message))
                if message.contains("MISSING")
        ));

        command_tx
            .send(Command::MonitorSetPrimary("DP-2".to_string()))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
                assert_eq!(layout.primary.as_deref(), Some("DP-2"));
            }
            other => panic!("expected a layout change, got {other:?}"),
        }

        task.abort();
    }

    #[tokio::test]
    async fn watch_events_keep_the_core_owned_primary() {
        let read = FakeMonitorBackend::new("read", MonitorCapabilities::OBSERVE);
        *read.read_result.lock().await = Ok(layout(vec![
            monitor("DP-1", 0, true),
            monitor("DP-2", 1, false),
        ]));
        let (watch_tx, watch_rx) = async_channel::unbounded();
        *read.watch_events.lock().await = Some(watch_rx);
        let read = read.arc();

        let service =
            MonitorService::with_candidates(vec![candidate("read", read)], vec![], fast_config());
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let initial = wait_for_snapshot(&event_rx).await;
        assert_eq!(initial.primary.as_deref(), Some("DP-1"));

        command_tx
            .send(Command::MonitorSetPrimary("DP-2".to_string()))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::LayoutChanged(_))
        ));

        watch_tx
            .send(layout(vec![
                monitor("DP-1", 0, true),
                monitor("DP-2", 1, false),
            ]))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
                assert_eq!(layout.primary.as_deref(), Some("DP-2"));
                assert!(layout.monitor("DP-2").unwrap().primary);
            }
            other => panic!("expected a layout change, got {other:?}"),
        }

        task.abort();
    }

    #[tokio::test]
    async fn a_removed_primary_falls_back_on_the_next_resync() {
        let read = FakeMonitorBackend::new("read", MonitorCapabilities::OBSERVE);
        *read.read_result.lock().await = Ok(layout(vec![
            monitor("DP-1", 0, false),
            monitor("DP-2", 1, true),
        ]));
        let (watch_tx, watch_rx) = async_channel::unbounded();
        *read.watch_events.lock().await = Some(watch_rx);
        let read = read.arc();

        let service =
            MonitorService::with_candidates(vec![candidate("read", read)], vec![], fast_config());
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let initial = wait_for_snapshot(&event_rx).await;
        assert_eq!(initial.primary.as_deref(), Some("DP-2"));

        command_tx
            .send(Command::MonitorSetPrimary("DP-2".to_string()))
            .await
            .unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap();

        watch_tx
            .send(layout(vec![monitor("DP-1", 0, true)]))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
                assert_eq!(layout.primary.as_deref(), Some("DP-1"));
            }
            other => panic!("expected a layout change, got {other:?}"),
        }

        task.abort();
    }

    #[tokio::test]
    async fn an_explicit_mode_is_written_and_recorded_for_cycling() {
        let mut read = FakeMonitorBackend::new("read", MonitorCapabilities::OBSERVE);
        *read.read_result.lock().await = Ok(layout(vec![
            monitor("DP-1", 0, true),
            monitor("DP-2", 1, false),
        ]));
        read.watch_delay = Some(Duration::from_secs(5));
        read.watch_result = Ok(());
        let read = read.arc();

        let write = FakeMonitorBackend::new(
            "write",
            MonitorCapabilities::WRITE | MonitorCapabilities::READ,
        );
        let write = write.arc();

        let service = MonitorService::with_candidates(
            vec![candidate("read", read)],
            vec![candidate("write", write.clone())],
            fast_config(),
        );
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        wait_for_snapshot(&event_rx).await;
        command_tx
            .send(Command::MonitorMode {
                target: "2".to_string(),
                mode: LayoutPreset::Mirror,
            })
            .await
            .unwrap();

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::Monitors(MonitorEvent::LayoutChanged(_))
        ));
        let applied = write.applied_layouts().await;
        assert_eq!(applied.len(), 1);
        assert_eq!(
            applied[0].monitor("DP-2").unwrap().mirror_of.as_deref(),
            Some("DP-1")
        );

        task.abort();
    }

    #[tokio::test]
    async fn a_recovered_specific_backend_wins_back_after_the_window() {
        let mut specific = FakeMonitorBackend::new("specific", MonitorCapabilities::OBSERVE);
        specific.probe_failures.store(1, Ordering::SeqCst);
        specific.watch_delay = Some(Duration::from_secs(30));
        specific.watch_result = Ok(());
        let specific = specific.arc();

        let mut fallback = FakeMonitorBackend::new("fallback", MonitorCapabilities::OBSERVE);
        fallback.watch_delay = Some(Duration::from_secs(30));
        fallback.watch_result = Ok(());
        let fallback = fallback.arc();

        let service = MonitorService::with_candidates(
            vec![
                candidate("specific", specific),
                candidate("fallback", fallback),
            ],
            vec![],
            fast_config(),
        );
        let (_command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let mut backends = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !backends.contains(&"specific") {
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Ok(Event::Monitors(MonitorEvent::BackendChanged { backend }))) => {
                    backends.push(backend);
                }
                Ok(Ok(_)) => {}
                _ => {}
            }
        }

        assert_eq!(backends.first().copied(), Some("fallback"));
        assert!(
            backends.contains(&"specific"),
            "the recovered backend did not take over: {backends:?}"
        );

        task.abort();
    }
}
