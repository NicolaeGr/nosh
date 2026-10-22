//! The idle inhibitor service.
//!
//! The service owns the desired active state. It connects a backend, forwards
//! user commands, publishes the state the compositor accepted, and reconnects
//! with the desired state after the connection is lost. The state is not
//! persisted: a process restart starts inactive.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::{Command, IdleInhibitCommand};
use crate::event::{Event, IdleInhibitEvent, Service, Snapshot};
use crate::idle_inhibit::backend::{IdleInhibitBackend, InhibitBackendEvent};
use crate::idle_inhibit::model::IdleInhibitState;
use crate::idle_inhibit::wayland::WaylandBackend;

#[derive(Debug, Clone)]
pub struct IdleInhibitServiceConfig {
    pub retry: RetryPolicy,
    /// A connection that lasted this long resets the backoff.
    pub stable_connection: Duration,
}

impl Default for IdleInhibitServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
        }
    }
}

pub struct IdleInhibitService {
    backend: Arc<dyn IdleInhibitBackend>,
    config: IdleInhibitServiceConfig,
}

impl IdleInhibitService {
    pub fn from_env() -> Self {
        Self::from_env_with(IdleInhibitServiceConfig::default())
    }

    pub fn from_env_with(config: IdleInhibitServiceConfig) -> Self {
        Self::with_backend(Arc::new(WaylandBackend::new()), config)
    }

    pub fn with_backend(
        backend: Arc<dyn IdleInhibitBackend>,
        config: IdleInhibitServiceConfig,
    ) -> Self {
        Self { backend, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let backend = self.backend;
        let mut backoff = Backoff::new(self.config.retry, seed());
        let mut desired_active = false;

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let session = match backend.connect(desired_active) {
                Ok(session) => session,
                Err(error) => {
                    tracing::debug!(
                        backend = backend.name(),
                        %error,
                        "cannot connect the idle inhibitor"
                    );
                    if report_unavailable(&events).await.is_err() {
                        return;
                    }
                    let delay = backoff.next_delay();
                    if wait_or_closed(&commands, &events, delay).await {
                        return;
                    }
                    continue;
                }
            };

            let session_events = session.events();
            let connected_at = Instant::now();
            let mut available = false;
            let mut active = false;

            loop {
                tokio::select! {
                    result = commands.recv() => {
                        let Ok(command) = result else {
                            return;
                        };
                        let Some(command) = command.idle_inhibit() else {
                            continue;
                        };
                        let requested = match command {
                            IdleInhibitCommand::Set(value) => value,
                            IdleInhibitCommand::Toggle => !active,
                        };

                        if !available {
                            desired_active = false;
                            tracing::warn!("the idle inhibitor is unavailable");
                            if events
                                .send(Event::IdleInhibit(IdleInhibitEvent::Unavailable))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            continue;
                        }

                        desired_active = requested;
                        if let Err(error) = session.set_active(requested) {
                            tracing::warn!(%error, "cannot change the idle inhibitor");
                            break;
                        }
                    }
                    event = session_events.recv() => {
                        match event {
                            Ok(InhibitBackendEvent::Connected {
                                available: connected_available,
                                active: connected_active,
                            }) => {
                                available = connected_available;
                                active = connected_active;
                                desired_active = connected_active;
                                if available {
                                    if emit_available(&events, backend.name(), active)
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                } else if report_unavailable(&events).await.is_err() {
                                    return;
                                }
                            }
                            Ok(InhibitBackendEvent::Availability(true)) => {
                                available = true;
                                if emit_available(&events, backend.name(), active)
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Ok(InhibitBackendEvent::Availability(false)) => {
                                available = false;
                                active = false;
                                desired_active = false;
                                if report_unavailable(&events).await.is_err() {
                                    return;
                                }
                            }
                            Ok(InhibitBackendEvent::ActiveChanged(value)) => {
                                active = value;
                                desired_active = value;
                                if events
                                    .send(Event::IdleInhibit(IdleInhibitEvent::Changed {
                                        active,
                                    }))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Ok(InhibitBackendEvent::Lost(reason)) => {
                                tracing::warn!(%reason, "the idle inhibitor connection was lost");
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }
            }

            drop(session);
            if report_unavailable(&events).await.is_err() {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the idle inhibitor");
            if wait_or_closed(&commands, &events, delay).await {
                return;
            }
        }
    }
}

async fn emit_available(
    events: &Sender<Event>,
    backend: &'static str,
    active: bool,
) -> Result<(), ()> {
    if events
        .send(Event::ServiceAvailability(Service::IdleInhibit, true))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::IdleInhibit(IdleInhibitEvent::Snapshot {
            available: true,
            active,
        }))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        idle_inhibit: Some(IdleInhibitState::new(true, active)),
        idle_inhibit_backend: Some(backend),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::IdleInhibit(IdleInhibitEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::IdleInhibit, false))
        .await
        .is_err()
    {
        return Err(());
    }
    Ok(())
}

/// Drops commands while disconnected and reports the inhibitor as unavailable.
async fn wait_or_closed(
    commands: &Receiver<Command>,
    events: &Sender<Event>,
    duration: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return false,
            result = commands.recv() => {
                match result {
                    Err(_) => return true,
                    Ok(command) => {
                        if command.is_idle_inhibit() {
                            tracing::warn!("the idle inhibitor is unavailable");
                            if events
                                .send(Event::IdleInhibit(IdleInhibitEvent::Unavailable))
                                .await
                                .is_err()
                            {
                                return true;
                            }
                        }
                    }
                }
            }
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use super::*;
    use crate::error::BackendError;
    use crate::idle_inhibit::backend::{InhibitBackendEvent, InhibitSession};

    #[derive(Clone)]
    struct FakeSession {
        events: Receiver<InhibitBackendEvent>,
        sender: Sender<InhibitBackendEvent>,
        applied: Arc<Mutex<Vec<bool>>>,
    }

    impl FakeSession {
        fn new() -> Self {
            let (sender, events) = async_channel::unbounded();
            Self {
                events,
                sender,
                applied: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn push(&self, event: InhibitBackendEvent) {
            let _ = self.sender.try_send(event);
        }

        fn applied(&self) -> Vec<bool> {
            self.applied.lock().unwrap().clone()
        }
    }

    impl InhibitSession for FakeSession {
        fn set_active(&self, active: bool) -> Result<(), BackendError> {
            self.applied.lock().unwrap().push(active);
            let _ = self
                .sender
                .try_send(InhibitBackendEvent::ActiveChanged(active));
            Ok(())
        }

        fn events(&self) -> Receiver<InhibitBackendEvent> {
            self.events.clone()
        }
    }

    struct FakeBackend {
        available: AtomicBool,
        failures: AtomicUsize,
        connects: AtomicUsize,
        initial: Mutex<Vec<bool>>,
        sessions: Mutex<Vec<FakeSession>>,
    }

    impl FakeBackend {
        fn new(available: bool) -> Self {
            Self {
                available: AtomicBool::new(available),
                failures: AtomicUsize::new(0),
                connects: AtomicUsize::new(0),
                initial: Mutex::new(Vec::new()),
                sessions: Mutex::new(Vec::new()),
            }
        }

        fn fail_first(&self, count: usize) {
            self.failures.store(count, Ordering::SeqCst);
        }

        fn last_session(&self) -> FakeSession {
            self.sessions
                .lock()
                .unwrap()
                .last()
                .expect("no session was created")
                .clone()
        }

        fn initial_states(&self) -> Vec<bool> {
            self.initial.lock().unwrap().clone()
        }
    }

    impl IdleInhibitBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn connect(&self, initial_active: bool) -> Result<Box<dyn InhibitSession>, BackendError> {
            let attempt = self.connects.fetch_add(1, Ordering::SeqCst);
            if attempt < self.failures.load(Ordering::SeqCst) {
                return Err(BackendError::Unavailable("not ready".to_string()));
            }

            self.initial.lock().unwrap().push(initial_active);
            let available = self.available.load(Ordering::SeqCst);
            let session = FakeSession::new();
            session.push(InhibitBackendEvent::Connected {
                available,
                active: initial_active && available,
            });
            self.sessions.lock().unwrap().push(session.clone());
            Ok(Box::new(session))
        }
    }

    fn service(backend: Arc<FakeBackend>) -> IdleInhibitService {
        IdleInhibitService::with_backend(
            backend,
            IdleInhibitServiceConfig {
                retry: RetryPolicy {
                    initial: Duration::from_millis(5),
                    max: Duration::from_millis(20),
                    jitter: false,
                },
                stable_connection: Duration::from_secs(5),
            },
        )
    }

    fn start(backend: Arc<FakeBackend>) -> (Sender<Command>, Receiver<Event>) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        tokio::spawn(service(backend).run(command_rx, events));
        (commands, event_rx)
    }

    async fn next_event(events: &Receiver<Event>) -> Event {
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("no event arrived")
            .expect("the event channel closed")
    }

    async fn next_idle_event(events: &Receiver<Event>) -> IdleInhibitEvent {
        loop {
            match next_event(events).await {
                Event::IdleInhibit(event) => return event,
                _ => continue,
            }
        }
    }

    async fn expect_snapshot(events: &Receiver<Event>, available: bool, active: bool) {
        match next_event(events).await {
            Event::Snapshot(snapshot) => {
                let state = snapshot.idle_inhibit.expect("the state was not merged");
                assert_eq!(state.available, available);
                assert_eq!(state.active, active);
                assert_eq!(snapshot.idle_inhibit_backend, Some("fake"));
            }
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    async fn consume_startup(events: &Receiver<Event>, available: bool) {
        assert!(matches!(
            next_event(events).await,
            Event::ServiceAvailability(Service::IdleInhibit, true)
        ));
        assert_eq!(
            next_idle_event(events).await,
            IdleInhibitEvent::Snapshot {
                available,
                active: false,
            }
        );
        expect_snapshot(events, available, false).await;
    }

    async fn wait_for_connects(backend: &FakeBackend, count: usize) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while backend.connects.load(Ordering::SeqCst) < count {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the service did not reconnect"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn startup_reports_the_protocol_and_an_inactive_inhibitor() {
        let backend = Arc::new(FakeBackend::new(true));
        let (_commands, events) = start(backend);

        consume_startup(&events, true).await;
    }

    #[tokio::test]
    async fn toggling_creates_and_destroys_the_inhibitor() {
        let backend = Arc::new(FakeBackend::new(true));
        let (commands, events) = start(backend.clone());
        consume_startup(&events, true).await;

        commands.send(Command::IdleInhibitToggle).await.unwrap();
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Changed { active: true }
        );

        commands.send(Command::IdleInhibitToggle).await.unwrap();
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Changed { active: false }
        );

        assert_eq!(backend.last_session().applied(), vec![true, false]);
    }

    #[tokio::test]
    async fn a_missing_protocol_stays_unavailable() {
        let backend = Arc::new(FakeBackend::new(false));
        let (commands, events) = start(backend.clone());

        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Unavailable
        );
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, false)
        ));

        commands.send(Command::IdleInhibitSet(true)).await.unwrap();
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Unavailable
        );
        assert!(backend.last_session().applied().is_empty());
    }

    #[tokio::test]
    async fn a_reconnect_reapplies_the_active_state() {
        let backend = Arc::new(FakeBackend::new(true));
        let (commands, events) = start(backend.clone());
        consume_startup(&events, true).await;

        commands.send(Command::IdleInhibitSet(true)).await.unwrap();
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Changed { active: true }
        );

        backend
            .last_session()
            .push(InhibitBackendEvent::Lost("compositor exited".to_string()));
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Unavailable
        );
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, false)
        ));

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, true)
        ));
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Snapshot {
                available: true,
                active: true,
            }
        );
        expect_snapshot(&events, true, true).await;
        assert_eq!(backend.initial_states(), vec![false, true]);
    }

    #[tokio::test]
    async fn a_protocol_that_appears_later_becomes_available() {
        let backend = Arc::new(FakeBackend::new(false));
        let (commands, events) = start(backend.clone());

        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Unavailable
        );
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, false)
        ));

        backend
            .last_session()
            .push(InhibitBackendEvent::Availability(true));
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, true)
        ));
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Snapshot {
                available: true,
                active: false,
            }
        );
        expect_snapshot(&events, true, false).await;

        commands.send(Command::IdleInhibitToggle).await.unwrap();
        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Changed { active: true }
        );
    }

    #[tokio::test]
    async fn a_failed_connection_retries_and_reports_unavailable() {
        let backend = Arc::new(FakeBackend::new(true));
        backend.fail_first(1);
        let (_commands, events) = start(backend.clone());

        assert_eq!(
            next_idle_event(&events).await,
            IdleInhibitEvent::Unavailable
        );
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::IdleInhibit, false)
        ));

        wait_for_connects(&backend, 2).await;
        consume_startup(&events, true).await;
    }
}
