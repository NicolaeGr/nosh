//! The battery service.
//!
//! Reads the UPower display device, publishes a snapshot at startup, and
//! re-reads the device on every property change. A missing UPower or a lost
//! connection reports the service as unavailable and retries with backoff.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::battery::backend::{
    resolve_battery, BatteryBackendEvent, BatteryBackendRef, BatteryCandidate,
};
use crate::battery::model::BatteryModel;
use crate::battery::upower::UpowerBackend;
use crate::command::Command;
use crate::event::{BatteryEvent, Event, Service, Snapshot};

#[derive(Debug, Clone)]
pub struct BatteryServiceConfig {
    pub retry: RetryPolicy,
    /// A connection that lasted this long resets the backoff.
    pub stable_connection: Duration,
}

impl Default for BatteryServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
        }
    }
}

pub struct BatteryService {
    candidates: Vec<BatteryCandidate>,
    config: BatteryServiceConfig,
}

impl BatteryService {
    pub fn from_env() -> Self {
        Self::from_env_with(BatteryServiceConfig::default())
    }

    pub fn from_env_with(config: BatteryServiceConfig) -> Self {
        let candidates = vec![BatteryCandidate::new("upower", || {
            Ok(std::sync::Arc::new(UpowerBackend::new()) as BatteryBackendRef)
        })];
        Self::with_candidates(candidates, config)
    }

    pub fn with_candidates(
        candidates: Vec<BatteryCandidate>,
        config: BatteryServiceConfig,
    ) -> Self {
        Self { candidates, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            let resolved = match resolve_battery("battery", self.candidates.clone()).await {
                Ok(resolved) => resolved,
                Err(error) => {
                    tracing::debug!(%error, "no battery backend is available");
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

            if events
                .send(Event::ServiceAvailability(Service::Battery, true))
                .await
                .is_err()
            {
                return;
            }

            let initial = match resolved.backend.read().await {
                Ok(properties) => properties.model(),
                Err(error) => {
                    tracing::warn!(
                        backend = resolved.name,
                        %error,
                        "the initial battery read failed"
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
            let backend = resolved.backend.clone();
            let mut watch = tokio::spawn(async move { backend.watch(watch_tx).await });
            let connected_at = Instant::now();

            loop {
                tokio::select! {
                    result = commands.recv() => {
                        // The battery accepts no commands. A closed channel
                        // means the core is shutting down.
                        if result.is_err() {
                            watch.abort();
                            return;
                        }
                    }
                    event = watch_rx.recv() => {
                        match event {
                            Ok(BatteryBackendEvent::Changed) => {
                                match resolved.backend.read().await {
                                    Ok(properties) => {
                                        let model = properties.model();
                                        if model != state {
                                            state = model;
                                            if events
                                                .send(Event::Battery(BatteryEvent::Changed(state.clone())))
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
                                            backend = resolved.name,
                                            %error,
                                            "a battery read after a change failed"
                                        );
                                        break;
                                    }
                                }
                            }
                            Ok(BatteryBackendEvent::Reconnected) => {
                                match resolved.backend.read().await {
                                    Ok(properties) => {
                                        state = properties.model();
                                        if emit_snapshot(&events, &state).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(
                                            backend = resolved.name,
                                            %error,
                                            "a battery read after a reconnect failed"
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
                                tracing::debug!(backend = resolved.name, "battery watch ended");
                            }
                            Ok(Err(error)) => {
                                tracing::warn!(backend = resolved.name, %error, "battery watch failed");
                            }
                            Err(error) => {
                                tracing::warn!(backend = resolved.name, %error, "battery watch task panicked");
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
            tracing::debug!(?delay, "reconnecting the battery backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

async fn emit_snapshot(events: &Sender<Event>, model: &BatteryModel) -> Result<(), ()> {
    if events
        .send(Event::Battery(BatteryEvent::Snapshot(model.clone())))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        battery: Some(model.clone()),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::Battery(BatteryEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::Battery, false))
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
                tracing::debug!("dropping a command while the battery is disconnected");
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
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::battery::backend::BatteryBackend;
    use crate::battery::model::{BatteryProperties, TYPE_BATTERY};
    use crate::error::BackendError;

    struct FakeBatteryBackend {
        probe_result: Result<(), BackendError>,
        watch_result: Result<(), BackendError>,
        hold_watch: bool,
        properties: Mutex<BatteryProperties>,
        events: Mutex<Option<Sender<BatteryBackendEvent>>>,
        probes: AtomicUsize,
        watches: AtomicUsize,
        watch_ready: Sender<()>,
        watch_ready_rx: async_channel::Receiver<()>,
    }

    impl FakeBatteryBackend {
        fn new(properties: BatteryProperties) -> Self {
            let (watch_ready, watch_ready_rx) = async_channel::unbounded();
            Self {
                probe_result: Ok(()),
                watch_result: Ok(()),
                hold_watch: true,
                properties: Mutex::new(properties),
                events: Mutex::new(None),
                probes: AtomicUsize::new(0),
                watches: AtomicUsize::new(0),
                watch_ready,
                watch_ready_rx,
            }
        }

        async fn set_percentage(&self, percentage: f64) {
            self.properties.lock().await.percentage = percentage;
        }

        async fn push_changed(&self) {
            self.push(BatteryBackendEvent::Changed).await;
        }

        async fn push_reconnected(&self) {
            self.push(BatteryBackendEvent::Reconnected).await;
        }

        async fn push(&self, event: BatteryBackendEvent) {
            let sender = self.events.lock().await.clone();
            if let Some(sender) = sender {
                let _ = sender.send(event).await;
            }
        }

        async fn wait_until_watching(&self) {
            let _ = tokio::time::timeout(Duration::from_secs(1), self.watch_ready_rx.recv()).await;
        }

        fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl BatteryBackend for FakeBatteryBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), BackendError> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            self.probe_result.clone()
        }

        async fn read(&self) -> Result<BatteryProperties, BackendError> {
            Ok(self.properties.lock().await.clone())
        }

        async fn watch(&self, events: Sender<BatteryBackendEvent>) -> Result<(), BackendError> {
            self.watches.fetch_add(1, Ordering::SeqCst);
            *self.events.lock().await = Some(events);
            let _ = self.watch_ready.send(()).await;
            if self.hold_watch {
                std::future::pending::<()>().await;
            }
            self.watch_result.clone()
        }
    }

    fn laptop(percentage: f64) -> BatteryProperties {
        BatteryProperties {
            kind: TYPE_BATTERY,
            percentage,
            state: 2,
            present: true,
            power_supply: true,
            time_to_empty: 3600,
            time_to_full: 0,
        }
    }

    fn service(backend: Arc<FakeBatteryBackend>) -> BatteryService {
        let candidate =
            BatteryCandidate::new("fake", move || Ok(backend.clone() as BatteryBackendRef));
        BatteryService::with_candidates(
            vec![candidate],
            BatteryServiceConfig {
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
        service: BatteryService,
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

    async fn next_battery_event(events: &async_channel::Receiver<Event>) -> BatteryEvent {
        loop {
            match next_event(events).await {
                Event::Battery(event) => return event,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_availability_and_a_snapshot() {
        let backend = FakeBatteryBackend::new(laptop(54.5)).arc();
        let (_commands, events) = start(service(backend));

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::Battery, true)
        ));

        let model = match next_battery_event(&events).await {
            BatteryEvent::Snapshot(model) => model,
            other => panic!("expected a snapshot, got {other:?}"),
        };

        assert!(model.present);
        assert!(model.is_battery);
        assert_eq!(model.percentage, 0.545);
        assert_eq!(model.icon_name, "battery-level-50-symbolic");
        assert_eq!(model.time_to_empty, Some(Duration::from_secs(3600)));
        assert_eq!(model.time_to_full, None);
        assert_eq!(
            model.tooltip(),
            "Battery: 54%\nDischarging · 1h 00m remaining"
        );
    }

    #[tokio::test]
    async fn a_property_change_emits_the_new_model() {
        let backend = FakeBatteryBackend::new(laptop(54.5)).arc();
        let (_commands, events) = start(service(backend.clone()));

        let _ = next_battery_event(&events).await;
        backend.wait_until_watching().await;

        backend.set_percentage(61.0).await;
        backend.push_changed().await;

        let model = match next_battery_event(&events).await {
            BatteryEvent::Changed(model) => model,
            other => panic!("expected a changed event, got {other:?}"),
        };
        assert_eq!(model.percentage, 0.61);
        assert_eq!(model.icon_name, "battery-level-60-symbolic");
    }

    #[tokio::test]
    async fn a_missing_battery_reports_unavailable_and_retries() {
        let mut fake = FakeBatteryBackend::new(laptop(50.0));
        fake.probe_result = Err(BackendError::Unavailable(
            "UPower reports no battery device".to_string(),
        ));
        let backend = fake.arc();
        let (commands, events) = start(service(backend.clone()));

        assert!(matches!(
            next_battery_event(&events).await,
            BatteryEvent::Unavailable
        ));
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::Battery, false)
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(backend.probes.load(Ordering::SeqCst) > 1);

        drop(commands);
    }

    #[tokio::test]
    async fn a_reconnect_emits_a_fresh_snapshot() {
        let backend = FakeBatteryBackend::new(laptop(54.5)).arc();
        let (_commands, events) = start(service(backend.clone()));

        let _ = next_battery_event(&events).await;
        backend.wait_until_watching().await;

        backend.set_percentage(70.0).await;
        backend.push_reconnected().await;

        let model = match next_battery_event(&events).await {
            BatteryEvent::Snapshot(model) => model,
            other => panic!("expected a fresh snapshot, got {other:?}"),
        };
        assert_eq!(model.percentage, 0.7);
    }

    #[tokio::test]
    async fn a_watch_failure_reconnects_and_resnapshots() {
        let mut fake = FakeBatteryBackend::new(laptop(40.0));
        fake.hold_watch = false;
        let backend = fake.arc();
        let (_commands, events) = start(service(backend.clone()));

        let _ = next_battery_event(&events).await;
        assert!(matches!(
            next_battery_event(&events).await,
            BatteryEvent::Unavailable
        ));
        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::Battery, false)
        ));

        let model = match next_battery_event(&events).await {
            BatteryEvent::Snapshot(model) => model,
            other => panic!("expected a fresh snapshot, got {other:?}"),
        };
        assert_eq!(model.percentage, 0.4);
        assert!(backend.watches.load(Ordering::SeqCst) >= 2);
    }
}
