//! The network service.
//!
//! NetworkManager is optional at build time and required for any network
//! control. A missing service reports the feature as unavailable and retries
//! with backoff, so the rest of quick settings keeps working.
//!
//! Power saving rules:
//!
//! - No polling. State arrives from D-Bus signals.
//! - Signal bursts are debounced into one read.
//! - The access point list is part of the model only while the network
//!   dropdown is open. `WifiSetDetails(false)` drops it again.
//! - `RequestScan` runs only on an explicit `WifiScan`, sent by the dropdown
//!   open handler and the refresh button.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_channel::{Receiver, Sender};

use crate::backoff::{Backoff, RetryPolicy};
use crate::command::Command;
use crate::event::{Event, NetworkEvent, Service, Snapshot};
use crate::network::backend::{NetworkBackendEvent, NetworkBackendRef};
use crate::network::model::NetworkModel;
use crate::network::nm::NetworkManagerBackend;

#[derive(Debug, Clone)]
pub struct NetworkServiceConfig {
    pub retry: RetryPolicy,
    /// A connection that lasted this long resets the backoff.
    pub stable_connection: Duration,
    /// Signal bursts inside this window collapse into one read.
    pub debounce: Duration,
    /// How long the scanning flag stays set when NetworkManager sends no
    /// completion signal.
    pub scan_timeout: Duration,
}

impl Default for NetworkServiceConfig {
    fn default() -> Self {
        Self {
            retry: RetryPolicy::default(),
            stable_connection: Duration::from_secs(5),
            debounce: Duration::from_millis(300),
            scan_timeout: Duration::from_secs(15),
        }
    }
}

pub struct NetworkService {
    backend: NetworkBackendRef,
    config: NetworkServiceConfig,
}

impl NetworkService {
    pub fn from_env() -> Self {
        Self::from_env_with(NetworkServiceConfig::default())
    }

    pub fn from_env_with(config: NetworkServiceConfig) -> Self {
        Self::with_backend(Arc::new(NetworkManagerBackend::new()), config)
    }

    pub fn with_backend(backend: NetworkBackendRef, config: NetworkServiceConfig) -> Self {
        Self { backend, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut backoff = Backoff::new(self.config.retry, seed());
        let (scan_done, scan_results) = async_channel::unbounded::<u64>();
        let mut generation = 0u64;

        loop {
            if commands.is_closed() || events.is_closed() {
                return;
            }

            if let Err(error) = self.backend.probe().await {
                tracing::debug!(
                    backend = self.backend.name(),
                    %error,
                    "NetworkManager is not available"
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
                .send(Event::ServiceAvailability(Service::Network, true))
                .await
                .is_err()
            {
                return;
            }
            tracing::info!(
                feature = "network",
                backend = self.backend.name(),
                "selected network backend"
            );

            // The overview always loads. Access points wait for the dropdown.
            let mut details = false;
            let mut scanning = false;
            let mut state = match self.backend.read(details).await {
                Ok(model) => model,
                Err(error) => {
                    tracing::warn!(
                        backend = self.backend.name(),
                        %error,
                        "the initial network read failed"
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
            state.wifi.scanning = scanning;
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

                        match command {
                            Command::WifiSetDetails(value) => {
                                if value == details {
                                    continue;
                                }
                                details = value;
                                scanning = false;
                                match self.backend.read(details).await {
                                    Ok(model) => {
                                        state = model;
                                        state.wifi.scanning = scanning;
                                        if emit_changed(&events, &state).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "a network read after a detail change failed");
                                    }
                                }
                            }
                            Command::WifiSetEnabled(enabled) => {
                                if let Err(error) = self.backend.set_wifi_enabled(enabled).await {
                                    tracing::warn!(%error, "setting Wi-Fi power failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Command::WifiScan => {
                                details = true;
                                generation = generation.wrapping_add(1);
                                let scan = generation;
                                match self.backend.scan_wifi().await {
                                    Ok(()) => {
                                        scanning = true;
                                        state.wifi.scanning = true;
                                        if events.send(Event::Network(NetworkEvent::ScanStarted)).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                        if emit_changed(&events, &state).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                        let timeout = self.config.scan_timeout;
                                        let done = scan_done.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(timeout).await;
                                            let _ = done.send(scan).await;
                                        });
                                    }
                                    Err(error) => {
                                        tracing::debug!(%error, "requesting a Wi-Fi scan failed");
                                        scanning = false;
                                        state.wifi.scanning = false;
                                        if send_failure(&events, error.to_string()).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                }
                            }
                            Command::WifiActivate { ssid, password } => {
                                match self
                                    .backend
                                    .activate_wifi(&ssid, password.as_deref(), None)
                                    .await
                                {
                                    Ok(()) => {
                                        // The connect result arrives through the watch.
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, ssid = %ssid, "connecting to Wi-Fi failed");
                                        if send_failure(&events, error.to_string()).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                }
                            }
                            Command::WifiDeactivate => {
                                if let Err(error) = self.backend.deactivate_wifi().await {
                                    tracing::warn!(%error, "disconnecting Wi-Fi failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Command::WiredSetEnabled { interface, enabled } => {
                                if let Err(error) = self
                                    .backend
                                    .set_wired_enabled(&interface, enabled)
                                    .await
                                {
                                    tracing::warn!(%error, %interface, "setting a wired interface failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                                // The disable flag is backend state, so refresh now.
                                if let Ok(model) = self.backend.read(details).await {
                                    state = model;
                                    state.wifi.scanning = scanning;
                                    if emit_changed(&events, &state).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Command::VpnActivate(uuid) => {
                                if let Err(error) = self.backend.activate_vpn(&uuid).await {
                                    tracing::warn!(%error, %uuid, "activating a VPN failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Command::VpnDeactivate(uuid) => {
                                if let Err(error) = self.backend.deactivate_vpn(&uuid).await {
                                    tracing::warn!(%error, %uuid, "deactivating a VPN failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Command::AirplaneModeSet(enabled) => {
                                if let Err(error) = self.backend.set_airplane_mode(enabled).await {
                                    tracing::warn!(%error, "setting airplane mode failed");
                                    if send_failure(&events, error.to_string()).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                                match self.backend.read(details).await {
                                    Ok(model) => {
                                        state = model;
                                        state.wifi.scanning = scanning;
                                        if emit_changed(&events, &state).await.is_err() {
                                            watch.abort();
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "a network read after airplane mode failed");
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    event = watch_rx.recv() => {
                        let Ok(event) = event else {
                            break;
                        };
                        if event == NetworkBackendEvent::Reconnected {
                            match self.backend.read(details).await {
                                Ok(model) => {
                                    state = model;
                                    state.wifi.scanning = scanning;
                                    if emit_snapshot(&events, &state).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "a network read after a reconnect failed");
                                    break;
                                }
                            }
                            continue;
                        }

                        // Collapse a burst of property changes into one read.
                        if !debounce(&watch_rx, self.config.debounce).await {
                            break;
                        }
                        match self.backend.read(details).await {
                            Ok(mut model) => {
                                model.wifi.scanning = scanning;
                                if model != state {
                                    state = model;
                                    if emit_changed(&events, &state).await.is_err() {
                                        watch.abort();
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                tracing::warn!(%error, "a network read after a change failed");
                                break;
                            }
                        }
                    }
                    scan = scan_results.recv() => {
                        let Ok(scan) = scan else {
                            break;
                        };
                        if scan != generation {
                            continue;
                        }
                        scanning = false;
                        match self.backend.read(details).await {
                            Ok(mut model) => {
                                model.wifi.scanning = false;
                                state = model;
                                if events.send(Event::Network(NetworkEvent::ScanFinished)).await.is_err() {
                                    watch.abort();
                                    return;
                                }
                                if emit_changed(&events, &state).await.is_err() {
                                    watch.abort();
                                    return;
                                }
                            }
                            Err(error) => {
                                tracing::warn!(%error, "a network read after a scan failed");
                                scanning = false;
                            }
                        }
                    }
                    result = &mut watch => {
                        log_watch_end(self.backend.name(), result);
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
            tracing::debug!(?delay, "reconnecting the network backend");
            if wait_or_closed(&commands, delay).await {
                return;
            }
        }
    }
}

/// Wait for the signal burst to settle. Returns false when the watch closed.
async fn debounce(events: &Receiver<NetworkBackendEvent>, duration: Duration) -> bool {
    let deadline = tokio::time::sleep(duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => return true,
            event = events.recv() => match event {
                Ok(NetworkBackendEvent::Changed) => continue,
                Ok(NetworkBackendEvent::Reconnected) => return true,
                Err(_) => return false,
            },
        }
    }
}

fn log_watch_end(
    name: &str,
    result: Result<Result<(), crate::error::BackendError>, tokio::task::JoinError>,
) {
    match result {
        Ok(Ok(())) => tracing::debug!(backend = name, "the network watch ended"),
        Ok(Err(error)) => tracing::warn!(backend = name, %error, "the network watch failed"),
        Err(error) => tracing::warn!(backend = name, %error, "the network watch task panicked"),
    }
}

async fn emit_snapshot(events: &Sender<Event>, model: &NetworkModel) -> Result<(), ()> {
    if events
        .send(Event::Network(NetworkEvent::Snapshot(model.clone())))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        network: Some(model.clone()),
        network_backend: Some("networkmanager"),
        ..Snapshot::default()
    };
    if events.send(Event::Snapshot(snapshot)).await.is_err() {
        return Err(());
    }
    Ok(())
}

async fn emit_changed(events: &Sender<Event>, model: &NetworkModel) -> Result<(), ()> {
    if events
        .send(Event::Network(NetworkEvent::Changed(model.clone())))
        .await
        .is_err()
    {
        return Err(());
    }
    let snapshot = Snapshot {
        network: Some(model.clone()),
        ..Snapshot::default()
    };
    events.send(Event::Snapshot(snapshot)).await.map_err(|_| ())
}

async fn send_failure(events: &Sender<Event>, message: String) -> Result<(), ()> {
    events
        .send(Event::Network(NetworkEvent::OperationFailed(message)))
        .await
        .map_err(|_| ())
}

async fn report_unavailable(events: &Sender<Event>) -> Result<(), ()> {
    if events
        .send(Event::Network(NetworkEvent::Unavailable))
        .await
        .is_err()
    {
        return Err(());
    }
    if events
        .send(Event::ServiceAvailability(Service::Network, false))
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
                tracing::debug!("dropping a command while NetworkManager is unavailable");
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
    use super::*;
    use crate::network::backend::NetworkBackend;
    use crate::network::model::WifiModel;
    use async_channel::Sender;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeBackend {
        model: Mutex<NetworkModel>,
        events: Mutex<Option<Sender<NetworkBackendEvent>>>,
        scans: Mutex<u32>,
        airplane: Mutex<Vec<bool>>,
        details: Mutex<Vec<bool>>,
        wired: Mutex<Vec<(String, bool)>>,
    }

    impl FakeBackend {
        async fn push(&self) {
            if let Some(events) = self.events.lock().unwrap().clone() {
                let _ = events.send(NetworkBackendEvent::Changed).await;
            }
        }

        fn arc(self) -> Arc<Self> {
            Arc::new(self)
        }
    }

    #[async_trait]
    impl NetworkBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn probe(&self) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        async fn read(&self, details: bool) -> Result<NetworkModel, crate::error::BackendError> {
            self.details.lock().unwrap().push(details);
            let mut model = self.model.lock().unwrap().clone();
            if !details {
                model.wifi.access_points.clear();
            }
            Ok(model)
        }

        async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), crate::error::BackendError> {
            self.model.lock().unwrap().wifi.enabled = enabled;
            Ok(())
        }

        async fn scan_wifi(&self) -> Result<(), crate::error::BackendError> {
            *self.scans.lock().unwrap() += 1;
            Ok(())
        }

        async fn activate_wifi(
            &self,
            _ssid: &str,
            _password: Option<&str>,
            _security: Option<crate::network::model::SecurityType>,
        ) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        async fn deactivate_wifi(&self) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        async fn set_wired_enabled(
            &self,
            interface: &str,
            enabled: bool,
        ) -> Result<(), crate::error::BackendError> {
            self.wired
                .lock()
                .unwrap()
                .push((interface.to_string(), enabled));
            Ok(())
        }

        async fn activate_vpn(&self, _uuid: &str) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        async fn deactivate_vpn(&self, _uuid: &str) -> Result<(), crate::error::BackendError> {
            Ok(())
        }

        async fn set_airplane_mode(&self, enabled: bool) -> Result<(), crate::error::BackendError> {
            self.airplane.lock().unwrap().push(enabled);
            self.model.lock().unwrap().airplane.enabled = enabled;
            Ok(())
        }

        async fn watch(
            &self,
            events: Sender<NetworkBackendEvent>,
        ) -> Result<(), crate::error::BackendError> {
            *self.events.lock().unwrap() = Some(events);
            std::future::pending().await
        }
    }

    fn model() -> NetworkModel {
        NetworkModel {
            available: true,
            wifi: WifiModel {
                available: true,
                enabled: true,
                state: crate::network::model::DeviceState::Activated,
                active_ssid: Some("home".to_string()),
                icon_name: "network-wireless-signal-good-symbolic".to_string(),
                ..WifiModel::default()
            },
            ..NetworkModel::default()
        }
    }

    fn service(backend: Arc<FakeBackend>) -> NetworkService {
        NetworkService::with_backend(
            backend,
            NetworkServiceConfig {
                debounce: Duration::from_millis(10),
                scan_timeout: Duration::from_secs(30),
                ..NetworkServiceConfig::default()
            },
        )
    }

    fn start(
        service: NetworkService,
    ) -> (
        Sender<Command>,
        Receiver<Event>,
        tokio::task::JoinHandle<()>,
    ) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let handle = tokio::spawn(service.run(command_rx, events));
        (commands, event_rx, handle)
    }

    async fn next_network(events: &Receiver<Event>) -> NetworkEvent {
        loop {
            match events.recv().await.unwrap() {
                Event::Network(event) => return event,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_a_snapshot_without_access_points() {
        let backend = FakeBackend {
            model: Mutex::new(model()),
            ..FakeBackend::default()
        }
        .arc();
        let (_commands, events, handle) = start(service(Arc::clone(&backend)));

        match next_network(&events).await {
            NetworkEvent::Snapshot(model) => assert!(model.wifi.access_points.is_empty()),
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(backend.details.lock().unwrap().as_slice(), [false]);
        handle.abort();
    }

    #[tokio::test]
    async fn a_scan_loads_details_and_marks_scanning() {
        let backend = FakeBackend {
            model: Mutex::new(model()),
            ..FakeBackend::default()
        }
        .arc();
        let (commands, events, handle) = start(service(Arc::clone(&backend)));
        let _ = next_network(&events).await;

        commands.send(Command::WifiScan).await.unwrap();

        assert!(matches!(
            next_network(&events).await,
            NetworkEvent::ScanStarted
        ));
        match next_network(&events).await {
            NetworkEvent::Changed(model) => assert!(model.wifi.scanning),
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(*backend.scans.lock().unwrap(), 1);
        // The access point list arrives with the next read after the scan.
        handle.abort();
    }

    #[tokio::test]
    async fn closing_the_dropdown_drops_the_access_point_list() {
        let backend = FakeBackend {
            model: Mutex::new(model()),
            ..FakeBackend::default()
        }
        .arc();
        let (commands, events, handle) = start(service(Arc::clone(&backend)));
        let _ = next_network(&events).await;

        commands.send(Command::WifiScan).await.unwrap();
        let _ = next_network(&events).await;
        let _ = next_network(&events).await;

        commands.send(Command::WifiSetDetails(false)).await.unwrap();
        match next_network(&events).await {
            NetworkEvent::Changed(model) => {
                assert!(model.wifi.access_points.is_empty());
                assert!(!model.wifi.scanning);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        handle.abort();
    }

    #[tokio::test]
    async fn airplane_mode_and_wired_commands_reach_the_backend() {
        let backend = FakeBackend {
            model: Mutex::new(model()),
            ..FakeBackend::default()
        }
        .arc();
        let (commands, events, handle) = start(service(Arc::clone(&backend)));
        let _ = next_network(&events).await;

        commands.send(Command::AirplaneModeSet(true)).await.unwrap();
        match next_network(&events).await {
            NetworkEvent::Changed(model) => assert!(model.airplane.enabled),
            other => panic!("unexpected event: {other:?}"),
        }

        commands
            .send(Command::WiredSetEnabled {
                interface: "eth0".to_string(),
                enabled: false,
            })
            .await
            .unwrap();
        let _ = next_network(&events).await;

        assert_eq!(*backend.airplane.lock().unwrap(), [true]);
        assert_eq!(
            backend.wired.lock().unwrap().as_slice(),
            [("eth0".to_string(), false)]
        );
        handle.abort();
    }

    #[tokio::test]
    async fn a_signal_burst_is_debounced_into_one_read() {
        let backend = FakeBackend {
            model: Mutex::new(model()),
            ..FakeBackend::default()
        }
        .arc();
        let (_commands, events, handle) = start(service(Arc::clone(&backend)));
        let _ = next_network(&events).await;

        // Wait for the watch to register its sender before emitting changes.
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if backend.events.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();

        let reads_before = backend.details.lock().unwrap().len();
        backend.push().await;
        backend.push().await;
        backend.push().await;

        // One read after the burst.
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if backend.details.lock().unwrap().len() > reads_before {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(backend.details.lock().unwrap().len(), reads_before + 1);
        handle.abort();
    }
}
