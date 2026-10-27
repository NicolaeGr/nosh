//! The power profiles service.
//!
//! Reads `power-profiles-daemon`, publishes a snapshot at startup, and
//! re-reads the daemon on every property change. The daemon is optional: a
//! missing service reports the feature as unavailable and retries with
//! backoff, so the rest of quick settings keeps working.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::event::{Event, PowerProfilesEvent, Service, Snapshot};
use crate::power_profiles::backend::{PowerProfilesBackendEvent, PowerProfilesBackendRef};
use crate::power_profiles::dbus::DbusPowerProfilesBackend;
use crate::power_profiles::model::{next_profile, PowerProfilesModel};

#[derive(Debug, Clone)]
pub struct PowerProfilesServiceConfig {
    pub retry: RetryPolicy,
    /// A connection that lasted this long resets the backoff.
    pub stable_connection: Duration,
}

impl Default for PowerProfilesServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
        }
    }
}

pub struct PowerProfilesService {
    backend: PowerProfilesBackendRef,
    config: PowerProfilesServiceConfig,
}

impl PowerProfilesService {
    pub fn from_env() -> Self {
        Self::from_env_with(PowerProfilesServiceConfig::default())
    }

    pub fn from_env_with(config: PowerProfilesServiceConfig) -> Self {
        Self::with_backend(Arc::new(DbusPowerProfilesBackend::new()), config)
    }

    pub fn with_backend(
        backend: PowerProfilesBackendRef,
        config: PowerProfilesServiceConfig,
    ) -> Self {
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
                    "the power profiles daemon is not available"
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
                .send(Event::ServiceAvailability(Service::PowerProfiles, true))
                .await
                .is_err()
            {
                return;
            }

            let initial = match self.backend.read().await {
                Ok(model) => model,
                Err(error) => {
                    tracing::warn!(
                        backend = self.backend.name(),
                        %error,
                        "the initial power profile read failed"
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

            let mut state = initial;
            if emit_snapshot(&events, &state).await.is_err() {
                return;
            }

            let (watch_tx, watch_rx) = async_channel::bounded(8);
            let backend = Arc::clone(&self.backend);
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();

            loop {
                tokio::select! {
                    result = commands.recv() => {
                        let Ok(command) = result else {
                            watch.abort();
                            return;
                        };
                        self.handle_command(command).await;
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(PowerProfilesBackendEvent::Changed) => {
                                match self.backend.read().await {
                                    Ok(model) => {
                                        if model != state {
                                            state = model;
                                            if events
                                                .send(Event::PowerProfiles(PowerProfilesEvent::Changed(
                                                    state.clone(),
                                                )))
                                                .await
                                                .is_err()
                                            {
                                                watch.abort();
                                                return;
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            backend = self.backend.name(),
                                            %error,
                                            "a power profile read after a change failed"
                                        );
                                        break;
                                    }
                                }
                            }
                            Ok(PowerProfilesBackendEvent::Reconnected) => {
                                match self.backend.read().await {
                                    Ok(model) => {
                                        state = model;
                                        if emit_snapshot(&events, &state).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            backend = self.backend.name(),
                                            %error,
                                            "a power profile read after a reconnect failed"
                                        );
                                        break;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    result = &mut watch => {
                        match result {
                            Ok(Ok(())) => {
                                tracing::debug!(
                                    backend = self.backend.name(),
                                    "the power profiles watch ended"
                                );
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(
                                    backend = self.backend.name(),
                                    %error,
                                    "the power profiles watch failed"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    backend = self.backend.name(),
                                    %error,
                                    "the power profiles watch task panicked"
                                );
                            }
                        }
                        break;
                    }
                }
            }

            watch.abort();
            if report_unavailable(&events).await.is_err() {
                return;
            }

            if connected_at.elapsed() >= self.config.stable_connection {
                backoff.reset();
            }
            let delay = backoff.next_delay();
            tracing::debug!(?delay, "reconnecting the power profiles backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }

    async fn handle_command(&self, command: Command) {
        let profile = match command {
            Command::PowerProfileSet(profile) => Some(profile),
            Command::PowerProfileCycle => match self.backend.read().await {
                Ok(model) => {
                    let next = next_profile(&model.active, &model.profiles);
                    if next.is_none() {
                        tracing::debug!("the daemon exposes no power profiles to cycle");
                    }
                    next
                }
                Err(error) => {
                    tracing::warn!(
                        backend = self.backend.name(),
                        %error,
                        "reading the power profile for a cycle failed"
                    );
                    return;
                }
            },
            _ => return,
        };

        let Some(profile) = profile else {
            return;
        };
        if let Err(error) = self.backend.set(&profile).await {
            tracing::warn!(
                backend = self.backend.name(),
                profile = %profile,
                %error,
                "setting the power profile failed"
            );
        }
    }
}

async fn emit_snapshot(events: &Sender<Event>, model: &PowerProfilesModel) -> Result<(), ()> {
    if events
        .send(Event::PowerProfiles(PowerProfilesEvent::Snapshot(
            model.clone(),
        )))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        power_profiles: Some(model.clone()),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::PowerProfiles(PowerProfilesEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::PowerProfiles, false))
        .await
        .is_err()
    {
        return Err(());
    }
    Ok(())
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a command while the power profiles daemon is unavailable");
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::error::BackendError;
    use crate::power_profiles::backend::PowerProfilesBackend;
    use crate::power_profiles::model::{default_profiles, PowerProfile};

    struct FakeBackend {
        probe_result: Result<(), BackendError>,
        hold_watch: bool,
        model: Mutex<PowerProfilesModel>,
        sets: Mutex<Vec<PowerProfile>>,
        events: Mutex<Option<Sender<PowerProfilesBackendEvent>>>,
        probes: AtomicUsize,
        watch_ready: Sender<()>,
        watch_ready_rx: async_channel::Receiver<()>,
    }

    impl FakeBackend {
        fn new(model: PowerProfilesModel) -> Self {
            let (watch_ready, watch_ready_rx) = async_channel::unbounded();
            Self {
                probe_result: Ok(()),
                hold_watch: true,
                model: Mutex::new(model),
                sets: Mutex::new(Vec::new()),
                events: Mutex::new(None),
                probes: AtomicUsize::new(0),
                watch_ready,
                watch_ready_rx,
            }
        }

        async fn set_model(&self, model: PowerProfilesModel) {
            *self.model.lock().await = model;
        }

        async fn push(&self, event: PowerProfilesBackendEvent) {
            let sender = self.events.lock().await.clone();
            if let Some(sender) = sender {
                let _ = sender.send(event).await;
            }
        }

        async fn wait_until_watching(&self) {
            let _ = tokio::time::timeout(Duration::from_secs(1), self.watch_ready_rx.recv()).await;
        }

        async fn sets(&self) -> Vec<PowerProfile> {
            self.sets.lock().await.clone()
        }

        fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl PowerProfilesBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), BackendError> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<PowerProfilesModel, BackendError> {
            Ok(self.model.lock().await.clone())
        }

        async fn set(&self, profile: &PowerProfile) -> Result<(), BackendError> {
            self.sets.lock().await.push(profile.clone());
            Ok(())
        }

        async fn watch(
            &self,
            events: Sender<PowerProfilesBackendEvent>,
        ) -> Result<(), BackendError> {
            *self.events.lock().await = Some(events);
            let _ = self.watch_ready.send(()).await;
            if self.hold_watch {
                std::future::pending::<()>().await;
            }
            Ok(())
        }
    }

    fn model(active: PowerProfile) -> PowerProfilesModel {
        PowerProfilesModel {
            active,
            profiles: default_profiles(),
            degraded: None,
        }
    }

    fn service(backend: Arc<FakeBackend>) -> PowerProfilesService {
        PowerProfilesService::with_backend(
            backend as PowerProfilesBackendRef,
            PowerProfilesServiceConfig {
                retry: RetryPolicy {
                    initial: Duration::from_millis(10),
                    max: Duration::from_millis(50),
                    jitter: false,
                },
                stable_connection: Duration::from_secs(5),
            },
        )
    }

    fn start(
        service: PowerProfilesService,
    ) -> (
        async_channel::Sender<Command>,
        async_channel::Receiver<Event>,
    ) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        tokio::spawn(service.run(command_rx, events));
        (commands, event_rx)
    }

    async fn next_event(events: &async_channel::Receiver<Event>) -> Event {
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("no event arrived")
            .expect("the event channel closed")
    }

    async fn next_power_event(events: &async_channel::Receiver<Event>) -> PowerProfilesEvent {
        loop {
            match next_event(events).await {
                Event::PowerProfiles(event) => return event,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_availability_and_a_snapshot() {
        let backend = FakeBackend::new(model(PowerProfile::Balanced)).arc();
        let (_commands, events) = start(service(backend));

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::PowerProfiles, true)
        ));

        let model = match next_power_event(&events).await {
            PowerProfilesEvent::Snapshot(model) => model,
            other => panic!("expected a snapshot, got {other:?}"),
        };
        assert_eq!(model.active, PowerProfile::Balanced);
        assert_eq!(model.profiles, default_profiles());
    }

    #[tokio::test]
    async fn a_property_change_emits_the_new_model() {
        let backend = FakeBackend::new(model(PowerProfile::Balanced)).arc();
        let (_commands, events) = start(service(Arc::clone(&backend)));

        let _ = next_power_event(&events).await;
        backend.wait_until_watching().await;

        backend.set_model(model(PowerProfile::Performance)).await;
        backend.push(PowerProfilesBackendEvent::Changed).await;

        let model = match next_power_event(&events).await {
            PowerProfilesEvent::Changed(model) => model,
            other => panic!("expected a changed event, got {other:?}"),
        };
        assert_eq!(model.active, PowerProfile::Performance);
    }

    #[tokio::test]
    async fn a_set_command_reaches_the_backend() {
        let backend = FakeBackend::new(model(PowerProfile::Balanced)).arc();
        let (commands, events) = start(service(Arc::clone(&backend)));

        let _ = next_power_event(&events).await;
        backend.wait_until_watching().await;

        commands
            .send(Command::PowerProfileSet(PowerProfile::Performance))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(backend.sets().await, vec![PowerProfile::Performance]);
    }

    #[tokio::test]
    async fn a_cycle_command_sets_the_next_profile() {
        let backend = FakeBackend::new(model(PowerProfile::PowerSaver)).arc();
        let (commands, events) = start(service(Arc::clone(&backend)));

        let _ = next_power_event(&events).await;
        backend.wait_until_watching().await;

        commands.send(Command::PowerProfileCycle).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(backend.sets().await, vec![PowerProfile::Balanced]);
    }

    #[tokio::test]
    async fn a_missing_daemon_reports_unavailable_and_retries() {
        let mut fake = FakeBackend::new(model(PowerProfile::Balanced));
        fake.probe_result = Err(BackendError::Unavailable("no daemon".to_string()));
        let backend = fake.arc();
        let (commands, events) = start(service(Arc::clone(&backend)));

        assert!(matches!(
            next_power_event(&events).await,
            PowerProfilesEvent::Unavailable
        ));
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::PowerProfiles, false)
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(backend.probes.load(Ordering::SeqCst) > 1);

        drop(commands);
    }
}
