use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};
use async_trait::async_trait;

use crate::backends::{BackendEvent, BackendRef, WorkspaceBackend};
use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::error::BackendError;
use crate::event::{Event, Service, Snapshot, WorkspaceEvent};
use crate::hyprland::HyprlandWorkspaceBackend;
use crate::model::WorkspaceCapabilities;
use crate::resolver::{resolve, Candidate, Resolved};
use crate::workspaces::state::WorkspaceState;

/// A backend that never produces workspaces. It keeps the shell running on a
/// compositor without a workspace adapter.
pub struct StaticWorkspaceBackend;

#[async_trait]
impl WorkspaceBackend for StaticWorkspaceBackend {
    fn name(&self) -> &'static str {
        "static"
    }

    async fn probe(&self) -> Result<WorkspaceCapabilities, BackendError> {
        Ok(WorkspaceCapabilities::BAR)
    }

    async fn read(&self) -> Result<WorkspaceState, BackendError> {
        Ok(WorkspaceState::default())
    }

    async fn activate(&self, _workspace: &str) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "activate",
        })
    }

    async fn move_to_monitor(&self, _workspace: &str, _monitor: &str) -> Result<(), BackendError> {
        Err(BackendError::Unsupported {
            backend: self.name(),
            capability: "move",
        })
    }

    async fn watch(&self, _events: Sender<BackendEvent>) -> Result<(), BackendError> {
        std::future::pending::<Result<(), BackendError>>().await
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceServiceConfig {
    pub required: WorkspaceCapabilities,
    pub missing_env_retry: Duration,
    pub retry: RetryPolicy,
    pub stable_connection: Duration,
    pub recovery_retry: RetryPolicy,
    pub recovery_stability: Duration,
    pub recovery_probe_interval: Duration,
}

impl Default for WorkspaceServiceConfig {
    fn default() -> Self {
        Self {
            required: WorkspaceCapabilities::BAR,
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

pub struct WorkspaceService {
    candidates: Vec<Candidate>,
    config: WorkspaceServiceConfig,
}

impl WorkspaceService {
    pub fn from_env() -> Self {
        Self::from_env_with(WorkspaceServiceConfig::default())
    }

    pub fn from_env_with(config: WorkspaceServiceConfig) -> Self {
        let hyprland = Candidate::new("hyprland", || match HyprlandWorkspaceBackend::detect() {
            Ok(Some(backend)) => Ok(Arc::new(backend) as BackendRef),
            Ok(None) => Err(BackendError::Unavailable(
                "HYPRLAND_INSTANCE_SIGNATURE is not set".to_string(),
            )),
            Err(error) => Err(BackendError::Unavailable(error.to_string())),
        });
        let static_backend = Candidate::new("static", || {
            Ok(Arc::new(StaticWorkspaceBackend) as BackendRef)
        });

        Self::with_candidates(vec![hyprland, static_backend], config)
    }

    pub fn with_candidates(candidates: Vec<Candidate>, config: WorkspaceServiceConfig) -> Self {
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
                match resolve("workspaces", self.candidates.clone(), self.config.required).await {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        tracing::warn!(%error, "no workspace backend is available");
                        if events
                            .send(Event::ServiceAvailability(Service::Workspaces, false))
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
                    .send(Event::Workspaces(WorkspaceEvent::BackendChanged {
                        backend: resolved.name,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            if events
                .send(Event::ServiceAvailability(Service::Workspaces, true))
                .await
                .is_err()
            {
                return;
            }

            let initial = match resolved.backend.read().await {
                Ok(state) => state,
                Err(error) => {
                    tracing::warn!(
                        backend = resolved.name,
                        %error,
                        "initial workspace read failed"
                    );
                    if emit_unavailable(&events).await.is_err() {
                        return;
                    }
                    if events
                        .send(Event::ServiceAvailability(Service::Workspaces, false))
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

            let snapshot = Snapshot {
                workspaces: Some(initial.snapshot()),
                workspace_backend: Some(resolved.name),
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

            loop {
                tokio::select! {
                    command = commands.recv() => {
                        match command {
                            Ok(command) => {
                                if let Err(error) =
                                    handle_command(&resolved.backend, command).await
                                {
                                    tracing::warn!(
                                        backend = resolved.name,
                                        %error,
                                        "workspace command failed"
                                    );
                                }
                            }
                            Err(_) => {
                                watch.abort();
                                return;
                            }
                        }
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(event) => {
                                if !forward_event(&events, event, resolved.name).await {
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
                                    "a more specific workspace backend recovered"
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
                                    "workspace watch ended"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "workspace watch failed"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    backend = resolved.name,
                                    %error,
                                    "workspace watch task panicked"
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
                .send(Event::ServiceAvailability(Service::Workspaces, false))
                .await
                .is_err()
            {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the workspace backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

async fn handle_command(backend: &BackendRef, command: Command) -> Result<(), BackendError> {
    match command {
        Command::WorkspaceActivate(workspace) => backend.activate(&workspace).await,
        Command::WorkspaceMoveToMonitor { workspace, monitor } => {
            backend.move_to_monitor(&workspace, &monitor).await
        }
    }
}

async fn forward_event(events: &Sender<Event>, event: BackendEvent, backend: &'static str) -> bool {
    let event = match event {
        BackendEvent::Snapshot(state) => Event::Workspaces(WorkspaceEvent::Snapshot {
            state: state.snapshot(),
            backend,
        }),
        BackendEvent::FocusedChanged(workspace) => {
            Event::Workspaces(WorkspaceEvent::FocusedChanged { workspace })
        }
        BackendEvent::UrgentChanged { workspace, urgent } => {
            Event::Workspaces(WorkspaceEvent::UrgentChanged { workspace, urgent })
        }
    };

    events.send(event).await.is_ok()
}

/// Re-probes the candidates that sit before the selected backend. A candidate
/// must pass repeated probes for the whole stability window before the service
/// switches back to it.
async fn recovery_loop(
    candidates: Vec<Candidate>,
    config: WorkspaceServiceConfig,
    signal: Sender<()>,
) {
    let mut backoff = Backoff::new(config.recovery_retry, seed());

    loop {
        let delay = backoff.next_delay();
        tracing::debug!(?delay, "re-probing more specific workspace backends");
        tokio::time::sleep(delay).await;

        let resolved = match resolve("workspaces", candidates.clone(), config.required).await {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::debug!(%error, "no more specific workspace backend is available");
                continue;
            }
        };

        tracing::debug!(
            backend = resolved.name,
            "specific workspace backend probed healthy"
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
                    "specific workspace backend is stable"
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
                    "specific workspace backend did not stay healthy"
                );
            }
        }
    }
}

async fn stable_backend(
    resolved: &Resolved,
    required: WorkspaceCapabilities,
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
                capability: "required workspace capabilities",
            });
        }
    }
}

async fn emit_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    events
        .send(Event::Workspaces(WorkspaceEvent::Unavailable))
        .await
        .map(|_| ())
        .map_err(|_| ())
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a workspace command while disconnected");
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
    use crate::backends::test_support::FakeWorkspaceBackend;

    fn candidate(name: &'static str, backend: Arc<FakeWorkspaceBackend>) -> Candidate {
        Candidate::new(name, move || Ok(backend.clone()))
    }

    fn fast_config() -> WorkspaceServiceConfig {
        WorkspaceServiceConfig {
            required: WorkspaceCapabilities::BAR,
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

    #[tokio::test]
    async fn the_static_backend_keeps_the_service_running_without_a_compositor() {
        let service = WorkspaceService::with_candidates(
            vec![
                Candidate::new("broken", || {
                    Err(BackendError::Unavailable("missing".into()))
                }),
                candidate(
                    "static",
                    FakeWorkspaceBackend::new("static", WorkspaceCapabilities::BAR).arc(),
                ),
            ],
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
            Event::Workspaces(WorkspaceEvent::BackendChanged { backend: "static" })
        ));

        let event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            event,
            Event::ServiceAvailability(Service::Workspaces, true)
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
        let mut backend = FakeWorkspaceBackend::new("fake", WorkspaceCapabilities::BAR);
        backend.watch_result = Err(BackendError::Unavailable("closed".into()));
        let backend = backend.arc();
        let service = WorkspaceService::with_candidates(
            vec![candidate("fake", backend.clone())],
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
        assert!(
            backend.watch_count() >= 2,
            "expected the watch loop to reconnect"
        );

        task.abort();
    }

    #[tokio::test]
    async fn a_command_reaches_the_selected_backend() {
        let mut backend = FakeWorkspaceBackend::new("fake", WorkspaceCapabilities::BAR);
        backend.read_result = Ok(WorkspaceState::default());
        backend.watch_result = Ok(());
        backend.watch_delay = Some(Duration::from_secs(5));
        let backend = backend.arc();
        let service = WorkspaceService::with_candidates(
            vec![candidate("fake", backend.clone())],
            fast_config(),
        );
        let (command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();

        let task = tokio::spawn(service.run(command_rx, event_tx));
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(Event::Snapshot(_)) = event_rx.recv().await {
                    break;
                }
            }
        })
        .await
        .unwrap();

        command_tx
            .send(Command::WorkspaceActivate("3".to_string()))
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if backend.state.lock().await.focused_workspace.as_deref() == Some("3") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        task.abort();
    }

    #[tokio::test]
    async fn a_recovered_specific_backend_wins_back_after_the_window() {
        let mut specific = FakeWorkspaceBackend::new("specific", WorkspaceCapabilities::BAR);
        specific.probe_failures.store(1, Ordering::SeqCst);
        specific.watch_delay = Some(Duration::from_secs(30));
        specific.watch_result = Ok(());
        let specific = specific.arc();

        let mut fallback = FakeWorkspaceBackend::new("fallback", WorkspaceCapabilities::BAR);
        fallback.watch_delay = Some(Duration::from_secs(30));
        fallback.watch_result = Ok(());
        let fallback = fallback.arc();

        let service = WorkspaceService::with_candidates(
            vec![
                candidate("specific", specific),
                candidate("fallback", fallback),
            ],
            fast_config(),
        );
        let (_command_tx, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let mut backends = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !backends.contains(&"specific") {
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Ok(Event::Workspaces(WorkspaceEvent::BackendChanged { backend }))) => {
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

    #[tokio::test]
    async fn a_backend_must_stay_healthy_for_the_whole_stability_window() {
        let backend = FakeWorkspaceBackend::new("specific", WorkspaceCapabilities::BAR).arc();
        let resolved = Resolved {
            backend: backend.clone(),
            name: "specific",
            capabilities: WorkspaceCapabilities::BAR,
            priority: 0,
        };
        let started = Instant::now();

        stable_backend(
            &resolved,
            WorkspaceCapabilities::BAR,
            Duration::from_millis(60),
            Duration::from_millis(10),
        )
        .await
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(60));
    }

    #[tokio::test]
    async fn a_failed_probe_aborts_the_stability_wait() {
        let backend = FakeWorkspaceBackend::new("specific", WorkspaceCapabilities::BAR);
        backend.probe_failures.store(1, Ordering::SeqCst);
        let backend = backend.arc();
        let resolved = Resolved {
            backend,
            name: "specific",
            capabilities: WorkspaceCapabilities::BAR,
            priority: 0,
        };

        let result = stable_backend(
            &resolved,
            WorkspaceCapabilities::BAR,
            Duration::from_millis(200),
            Duration::from_millis(10),
        )
        .await;

        assert!(result.is_err());
    }
}
