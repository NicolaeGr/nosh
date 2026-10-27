//! The brightness service.
//!
//! The service owns the device list, the last known value, and the command
//! queue. Every write runs here, one at a time, so a slow DDC/CI call cannot
//! block the GTK thread, the Tokio runtime, or another display. Slider movement
//! is merged per device before it reaches a backend.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};

use crate::brightness::backend::{
    discover_first, resolve_device, BrightnessBackendRef, BrightnessCandidate,
};
use crate::brightness::brightnessctl::BrightnessctlBackend;
use crate::brightness::command::{CommandBackend, CommandDeviceConfig};
use crate::brightness::ddc::{DdcBackendBridge, DdcutilBackend, DEFAULT_DDC_TIMEOUT};
use crate::brightness::model::{
    clamp_percent, linear_raw_from_percent, raw_from_percent, BrightnessDevice, BrightnessDeviceId,
    BrightnessDeviceInfo, BrightnessKind, BrightnessSnapshotFile, PERCENT_EPSILON,
};
use crate::brightness::snapshot;
use crate::brightness::sysfs::SysfsLogindBackend;
use crate::command::Command;
use crate::error::BrightnessError;
use crate::event::{BrightnessEvent, Event, Service, Snapshot};
use crate::origin::ChangeOrigin;
use crate::paths::NoshPaths;

#[derive(Debug, Clone)]
pub struct BrightnessServiceConfig {
    /// Merges slider movement into one backend write per device.
    pub debounce: Duration,
    pub command_timeout: Duration,
    pub ddc_timeout: Duration,
    /// How often internal brightness files are checked for external changes.
    pub poll_interval: Duration,
    /// How often an unavailable DDC display is looked for again.
    pub ddc_retry_interval: Duration,
    pub keep_snapshot: bool,
    pub snapshot_path: Option<PathBuf>,
    pub command_devices: Vec<CommandDeviceConfig>,
}

impl Default for BrightnessServiceConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(50),
            command_timeout: Duration::from_secs(3),
            ddc_timeout: DEFAULT_DDC_TIMEOUT,
            poll_interval: Duration::from_secs(2),
            ddc_retry_interval: Duration::from_secs(30),
            keep_snapshot: false,
            snapshot_path: None,
            command_devices: Vec::new(),
        }
    }
}

pub struct BrightnessService {
    internal: Vec<BrightnessCandidate>,
    external: Vec<BrightnessCandidate>,
    command: Vec<BrightnessCandidate>,
    config: BrightnessServiceConfig,
}

impl BrightnessService {
    pub fn from_env() -> Self {
        Self::from_env_with(BrightnessServiceConfig::default())
    }

    pub fn from_env_with(mut config: BrightnessServiceConfig) -> Self {
        if config.snapshot_path.is_none() {
            if let Ok(paths) = NoshPaths::from_env() {
                config.snapshot_path = Some(paths.brightness_snapshot());
            }
        }

        let internal = vec![
            BrightnessCandidate::new("sysfs-logind", || {
                Ok(Arc::new(SysfsLogindBackend::new()) as BrightnessBackendRef)
            }),
            BrightnessCandidate::new("brightnessctl", || {
                Ok(Arc::new(BrightnessctlBackend::new()) as BrightnessBackendRef)
            }),
        ];

        let ddc_timeout = config.ddc_timeout;
        let external = vec![BrightnessCandidate::new("ddcutil", move || {
            let backend = DdcutilBackend::with_config("ddcutil", ddc_timeout);
            if !backend.available() {
                return Err(BrightnessError::Unavailable(
                    "ddcutil is not installed".to_string(),
                ));
            }
            Ok(Arc::new(DdcBackendBridge::new(Arc::new(backend))) as BrightnessBackendRef)
        })];

        let command = if config.command_devices.is_empty() {
            Vec::new()
        } else {
            let devices = config.command_devices.clone();
            let timeout = config.command_timeout;
            vec![BrightnessCandidate::new("command", move || {
                Ok(
                    Arc::new(CommandBackend::with_config(devices.clone(), "sh", timeout))
                        as BrightnessBackendRef,
                )
            })]
        };

        Self::with_candidates(internal, external, command, config)
    }

    pub fn with_candidates(
        internal: Vec<BrightnessCandidate>,
        external: Vec<BrightnessCandidate>,
        command: Vec<BrightnessCandidate>,
        config: BrightnessServiceConfig,
    ) -> Self {
        Self {
            internal,
            external,
            command,
            config,
        }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let (discovery_tx, discovery_rx) = async_channel::unbounded();
        let mut state = BrightnessState::default();

        self.discover_local(&mut state).await;
        if emit_snapshot(&events, &state).await.is_err() {
            return;
        }

        self.spawn_external_discovery(&discovery_tx);

        let mut pending: BTreeMap<BrightnessDeviceId, f64> = BTreeMap::new();
        let mut deadline: Option<tokio::time::Instant> = None;
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut retry_ticker = tokio::time::interval(self.config.ddc_retry_interval);
        retry_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        retry_ticker.tick().await;

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
                            if self
                                .handle_command(
                                    command,
                                    &mut state,
                                    &mut pending,
                                    &mut deadline,
                                    &events,
                                    &discovery_tx,
                                )
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
                _ = debounce, if !pending.is_empty() => {
                    deadline = None;
                    if flush_pending(&mut state, &mut pending, &events).await.is_err() {
                        return;
                    }
                }
                discovered = discovery_rx.recv() => {
                    let Ok(devices) = discovered else {
                        return;
                    };
                    if apply_external(&mut state, devices)
                        && emit_snapshot(&events, &state).await.is_err()
                    {
                        return;
                    }
                }
                _ = ticker.tick() => {
                    if self.poll(&mut state, &events).await.is_err() {
                        return;
                    }
                }
                _ = retry_ticker.tick() => {
                    let retry = state.devices.iter().any(|device| {
                        device.device.kind == BrightnessKind::ExternalDdc
                            && !device.device.available
                    });
                    if retry {
                        self.spawn_external_discovery(&discovery_tx);
                    }
                }
            }
        }
    }

    async fn discover_local(&self, state: &mut BrightnessState) {
        let discovered = discover_first(&self.internal).await;
        for info in discovered.into_iter().take(1) {
            add_or_rebind(state, &self.internal, info).await;
        }

        for info in discover_first(&self.command).await {
            add_or_rebind(state, &self.command, info).await;
        }
    }

    fn spawn_external_discovery(&self, sender: &Sender<Vec<BoundDevice>>) {
        if self.external.is_empty() {
            return;
        }
        let candidates = self.external.clone();
        let sender = sender.clone();
        tokio::spawn(async move {
            // DDC reads are slow. Bind every display here so the service only
            // has to insert the finished devices.
            let infos = discover_first(&candidates).await;
            let mut bound = Vec::with_capacity(infos.len());
            for info in infos {
                if let Some(device) = bind_device(info, &candidates).await {
                    bound.push(device);
                }
            }
            let _ = sender.send(bound).await;
        });
    }

    async fn handle_command(
        &self,
        command: Command,
        state: &mut BrightnessState,
        pending: &mut BTreeMap<BrightnessDeviceId, f64>,
        deadline: &mut Option<tokio::time::Instant>,
        events: &Sender<Event>,
        discovery_tx: &Sender<Vec<BoundDevice>>,
    ) -> Result<(), ()> {
        match command {
            Command::BrightnessSet(percent) => {
                let targets = state
                    .devices
                    .iter()
                    .filter(|device| device.device.available)
                    .map(|device| (device.device.id.clone(), clamp_percent(percent)))
                    .collect();
                schedule(
                    state,
                    pending,
                    deadline,
                    self.config.debounce,
                    targets,
                    events,
                )
                .await
            }
            Command::BrightnessStep(delta) => {
                // A step becomes an absolute target before it enters the queue.
                // A queued value is the base so two rapid steps move twice.
                let targets = state
                    .devices
                    .iter()
                    .filter(|device| device.device.available)
                    .map(|device| {
                        let base = device
                            .device
                            .pending_percent
                            .unwrap_or(device.device.percent);
                        (device.device.id.clone(), clamp_percent(base + delta))
                    })
                    .collect();
                schedule(
                    state,
                    pending,
                    deadline,
                    self.config.debounce,
                    targets,
                    events,
                )
                .await
            }
            Command::BrightnessSetTarget { device, percent } => {
                if state.index(&device).is_none() {
                    tracing::warn!(device = %device, "unknown brightness target");
                    return Ok(());
                }
                schedule(
                    state,
                    pending,
                    deadline,
                    self.config.debounce,
                    vec![(device, clamp_percent(percent))],
                    events,
                )
                .await
            }
            other => {
                // Snapshot, Dim, and Restore must not overtake a queued write.
                if !pending.is_empty() {
                    *deadline = None;
                    flush_pending(state, pending, events).await?;
                }

                match other {
                    Command::BrightnessSnapshot => {
                        capture_snapshot(state, &self.config, events).await
                    }
                    Command::BrightnessDim { value } => {
                        dim(state, &self.config, value, events).await
                    }
                    Command::BrightnessRestore => restore(state, &self.config, events).await,
                    Command::BrightnessRescan => {
                        self.discover_local(state).await;
                        self.spawn_external_discovery(discovery_tx);
                        emit_snapshot(events, state).await
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    /// Reads internal brightness files and picks up sysfs backlight changes.
    async fn poll(&self, state: &mut BrightnessState, events: &Sender<Event>) -> Result<(), ()> {
        let mut changed = false;

        for device in state.devices.iter_mut() {
            if !device.device.available
                || !device.device.kind.is_internal()
                || device.device.backend != Some("sysfs-logind")
                || device.device.pending_percent.is_some()
            {
                continue;
            }

            match device.backend.read(&device.device.id).await {
                Ok(reading) => {
                    let old = device.device.percent;
                    if (reading.percent - old).abs() >= PERCENT_EPSILON {
                        device.device.percent = reading.percent;
                        device.device.raw = reading.raw;
                        device.device.max_raw = reading.max_raw;
                        changed = true;
                        if events
                            .send(Event::Brightness(BrightnessEvent::DeviceChanged {
                                device: device.device.id.clone(),
                                percent: reading.percent,
                                old,
                                origin: ChangeOrigin::External,
                            }))
                            .await
                            .is_err()
                        {
                            return Err(());
                        }
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        device = %device.device.id,
                        %error,
                        "cannot poll the internal backlight"
                    );
                }
            }
        }

        let discovered = discover_first(&self.internal).await;
        let present: Vec<BrightnessDeviceId> =
            discovered.iter().map(|info| info.id.clone()).collect();

        if let Some(info) = discovered.into_iter().next() {
            changed |= add_or_rebind(state, &self.internal, info).await;
        }

        for device in state.devices.iter_mut() {
            if device.device.kind.is_internal()
                && device.device.available
                && !present.contains(&device.device.id)
            {
                device.device.available = false;
                device.device.pending_percent = None;
                changed = true;
                tracing::info!(
                    device = %device.device.id,
                    "internal brightness device disappeared"
                );
            }
        }

        if changed {
            emit_snapshot(events, state).await
        } else {
            Ok(())
        }
    }
}

struct BoundDevice {
    device: BrightnessDevice,
    backend: BrightnessBackendRef,
    priority: usize,
    candidates: Vec<BrightnessCandidate>,
}

#[derive(Default)]
struct BrightnessState {
    devices: Vec<BoundDevice>,
}

impl BrightnessState {
    fn index(&self, id: &BrightnessDeviceId) -> Option<usize> {
        self.devices
            .iter()
            .position(|device| &device.device.id == id)
    }

    fn models(&self) -> Vec<BrightnessDevice> {
        self.devices
            .iter()
            .map(|device| device.device.clone())
            .collect()
    }

    fn has_available(&self) -> bool {
        self.devices.iter().any(|device| device.device.available)
    }

    fn all_unavailable(&self) -> bool {
        !self.devices.is_empty() && self.devices.iter().all(|device| !device.device.available)
    }

    fn available_ids(&self) -> Vec<BrightnessDeviceId> {
        self.devices
            .iter()
            .filter(|device| device.device.available)
            .map(|device| device.device.id.clone())
            .collect()
    }
}

async fn add_or_rebind(
    state: &mut BrightnessState,
    candidates: &[BrightnessCandidate],
    info: BrightnessDeviceInfo,
) -> bool {
    let existing = state.index(&info.id);
    if let Some(index) = existing {
        if state.devices[index].device.available {
            return false;
        }
        // A device that came back gets a fresh backend and its current value.
        if let Some(bound) = bind_device(info, candidates).await {
            state.devices[index] = bound;
            return true;
        }
        return false;
    }

    if let Some(bound) = bind_device(info, candidates).await {
        state.devices.push(bound);
        return true;
    }
    false
}

async fn bind_device(
    info: BrightnessDeviceInfo,
    candidates: &[BrightnessCandidate],
) -> Option<BoundDevice> {
    let bound = resolve_device(&info, candidates).await?;
    let reading = bound.reading;

    Some(BoundDevice {
        device: BrightnessDevice {
            id: info.id,
            kind: info.kind,
            name: info.name,
            connector: info.connector,
            percent: reading.percent,
            pending_percent: None,
            available: true,
            raw: reading.raw,
            max_raw: reading.max_raw,
            ddc_bus: info.ddc_bus,
            backend: Some(bound.name),
        },
        backend: bound.backend,
        priority: bound.priority,
        candidates: candidates.to_vec(),
    })
}

/// Adds external displays discovered in the background and retires devices that
/// disappeared.
fn apply_external(state: &mut BrightnessState, devices: Vec<BoundDevice>) -> bool {
    let found: Vec<BrightnessDeviceId> = devices
        .iter()
        .map(|device| device.device.id.clone())
        .collect();
    let mut changed = false;

    for bound in devices {
        match state.index(&bound.device.id) {
            Some(index) => {
                let existing = &mut state.devices[index];
                if !existing.device.available {
                    *existing = bound;
                    changed = true;
                } else if existing.device.pending_percent.is_none()
                    && (bound.device.percent - existing.device.percent).abs() >= PERCENT_EPSILON
                {
                    existing.device.percent = bound.device.percent;
                    existing.device.raw = bound.device.raw;
                    existing.device.max_raw = bound.device.max_raw;
                    existing.device.backend = bound.device.backend;
                    changed = true;
                }
            }
            None => {
                state.devices.push(bound);
                changed = true;
            }
        }
    }

    for device in state.devices.iter_mut() {
        if device.device.kind == BrightnessKind::ExternalDdc
            && device.device.available
            && !found.contains(&device.device.id)
        {
            device.device.available = false;
            device.device.pending_percent = None;
            changed = true;
            tracing::info!(
                device = %device.device.id,
                "external brightness device disappeared"
            );
        }
    }

    changed
}

async fn schedule(
    state: &mut BrightnessState,
    pending: &mut BTreeMap<BrightnessDeviceId, f64>,
    deadline: &mut Option<tokio::time::Instant>,
    debounce: Duration,
    targets: Vec<(BrightnessDeviceId, f64)>,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let mut any = false;

    for (id, target) in targets {
        if let Some(index) = state.index(&id) {
            state.devices[index].device.pending_percent = Some(target);
        }
        pending.insert(id.clone(), target);
        if events
            .send(Event::Brightness(BrightnessEvent::Pending {
                device: id,
                target,
            }))
            .await
            .is_err()
        {
            return Err(());
        }
        any = true;
    }

    if any {
        *deadline = Some(tokio::time::Instant::now() + debounce);
    } else if state.devices.is_empty() {
        tracing::debug!("no brightness device is available for a set command");
    }

    Ok(())
}

async fn flush_pending(
    state: &mut BrightnessState,
    pending: &mut BTreeMap<BrightnessDeviceId, f64>,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let writes: Vec<(BrightnessDeviceId, f64)> = pending
        .iter()
        .map(|(id, value)| (id.clone(), *value))
        .collect();
    pending.clear();

    for (id, target) in writes {
        apply_target(state, &id, target, ChangeOrigin::User, events).await?;
    }
    Ok(())
}

/// Writes one device. Returns whether the write succeeded. `Err` means the event
/// channel closed.
async fn apply_target(
    state: &mut BrightnessState,
    id: &BrightnessDeviceId,
    target: f64,
    origin: ChangeOrigin,
    events: &Sender<Event>,
) -> Result<bool, ()> {
    let Some(index) = state.index(id) else {
        return Ok(false);
    };
    let old = state.devices[index].device.percent;

    match write_with_fallback(state, index, target).await {
        Ok(()) => {
            let device = &mut state.devices[index];
            device.device.percent = target;
            device.device.pending_percent = None;
            device.device.available = true;
            device.device.raw = device.device.max_raw.map(|max| {
                if device.device.kind.is_internal() {
                    raw_from_percent(target, max)
                } else {
                    linear_raw_from_percent(target, max)
                }
            });

            events
                .send(Event::Brightness(BrightnessEvent::DeviceChanged {
                    device: id.clone(),
                    percent: target,
                    old,
                    origin,
                }))
                .await
                .map_err(|_| ())?;

            emit_snapshot(events, state).await?;
            Ok(true)
        }
        Err(error) => {
            tracing::warn!(device = %id, %error, "brightness write failed");

            // `DDCRC_VERIFY` means ddcutil sent the write but the monitor did not
            // confirm it. If the display still answers a read, keep it available
            // instead of hiding the slider until the next scan.
            if state.devices[index].device.kind == BrightnessKind::ExternalDdc {
                let read = state.devices[index].backend.read(id).await;
                if let Ok(reading) = read {
                    let device = &mut state.devices[index];
                    device.device.pending_percent = None;
                    device.device.available = true;
                    device.device.percent = reading.percent;
                    device.device.raw = reading.raw;
                    device.device.max_raw = reading.max_raw;
                    tracing::info!(
                        device = %id,
                        percent = reading.percent,
                        "brightness write did not verify; the display still responds"
                    );
                    emit_snapshot(events, state).await?;
                    return Ok(false);
                }
            }

            let device = &mut state.devices[index];
            device.device.pending_percent = None;
            device.device.available = false;

            if state.all_unavailable() {
                tracing::warn!("every brightness device is unavailable");
                events
                    .send(Event::Brightness(BrightnessEvent::Unavailable))
                    .await
                    .map_err(|_| ())?;
            }
            emit_snapshot(events, state).await?;
            Ok(false)
        }
    }
}

async fn write_with_fallback(
    state: &mut BrightnessState,
    index: usize,
    target: f64,
) -> Result<(), BrightnessError> {
    loop {
        let (backend, id, priority, candidates) = {
            let device = &state.devices[index];
            (
                device.backend.clone(),
                device.device.id.clone(),
                device.priority,
                device.candidates.clone(),
            )
        };

        match backend.write(&id, target).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let mut rebound = None;
                for (next_priority, candidate) in candidates.iter().enumerate().skip(priority + 1) {
                    let Ok(candidate_backend) = candidate.create() else {
                        continue;
                    };
                    if !candidate_backend.supports(&id) {
                        continue;
                    }
                    if candidate_backend.probe().await.is_err() {
                        continue;
                    }
                    match candidate_backend.read(&id).await {
                        Ok(reading) => {
                            rebound =
                                Some((candidate_backend, candidate.name, next_priority, reading));
                            break;
                        }
                        Err(fallback_error) => {
                            tracing::debug!(
                                backend = candidate.name,
                                device = %id,
                                %fallback_error,
                                "brightness fallback read failed"
                            );
                        }
                    }
                }

                let Some((fallback, name, next_priority, reading)) = rebound else {
                    return Err(error);
                };

                tracing::warn!(
                    device = %id,
                    from = ?state.devices[index].device.backend,
                    to = name,
                    "brightness backend failed; falling back"
                );
                let device = &mut state.devices[index];
                device.backend = fallback;
                device.priority = next_priority;
                device.device.backend = Some(name);
                device.device.raw = reading.raw;
                device.device.max_raw = reading.max_raw;
            }
        }
    }
}

async fn capture_snapshot(
    state: &mut BrightnessState,
    config: &BrightnessServiceConfig,
    events: &Sender<Event>,
) -> Result<(), ()> {
    refresh_readings(state).await;

    let available: Vec<BrightnessDevice> = state
        .models()
        .into_iter()
        .filter(|device| device.available)
        .collect();
    let file = BrightnessSnapshotFile::from_devices(&available, snapshot::now());
    match &config.snapshot_path {
        Some(path) => {
            if let Err(error) = snapshot::save(path, &file).await {
                tracing::warn!(%error, "cannot write the brightness snapshot");
            }
        }
        None => tracing::warn!("no brightness snapshot path is configured"),
    }

    emit_snapshot(events, state).await
}

async fn refresh_readings(state: &mut BrightnessState) {
    for device in state.devices.iter_mut() {
        if !device.device.available {
            continue;
        }
        match device.backend.read(&device.device.id).await {
            Ok(reading) => {
                device.device.percent = reading.percent;
                device.device.raw = reading.raw;
                device.device.max_raw = reading.max_raw;
            }
            Err(error) => {
                tracing::debug!(
                    device = %device.device.id,
                    %error,
                    "cannot read a brightness device for the snapshot"
                );
            }
        }
    }
}

async fn dim(
    state: &mut BrightnessState,
    config: &BrightnessServiceConfig,
    value: f64,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let existing = match &config.snapshot_path {
        Some(path) => snapshot::load(path).await.ok().flatten().is_some(),
        None => false,
    };
    if !existing {
        capture_snapshot(state, config, events).await?;
    }

    let target = clamp_percent(value);
    for id in state.available_ids() {
        apply_target(state, &id, target, ChangeOrigin::Automatic, events).await?;
    }
    emit_snapshot(events, state).await
}

async fn restore(
    state: &mut BrightnessState,
    config: &BrightnessServiceConfig,
    events: &Sender<Event>,
) -> Result<(), ()> {
    let Some(path) = &config.snapshot_path else {
        tracing::warn!("no brightness snapshot path is configured");
        return Ok(());
    };
    let Some(file) = snapshot::load(path).await.map_err(|error| {
        tracing::warn!(%error, "cannot read the brightness snapshot");
    })?
    else {
        tracing::info!("there is no brightness snapshot to restore");
        return Ok(());
    };

    let mut missing = false;
    let mut all_writes_ok = true;

    for entry in &file.devices {
        let Some(index) = find_entry(state, entry) else {
            missing = true;
            tracing::warn!(
                device = %entry.id,
                "the snapshot device is not present; keeping its entry"
            );
            continue;
        };
        if !state.devices[index].device.available {
            missing = true;
            tracing::warn!(
                device = %entry.id,
                "the snapshot device is unavailable; keeping its entry"
            );
            continue;
        }

        let id = state.devices[index].device.id.clone();
        let applied = apply_target(
            state,
            &id,
            clamp_percent(entry.percent),
            ChangeOrigin::Restore,
            events,
        )
        .await?;
        all_writes_ok &= applied;
    }

    if !missing && all_writes_ok && !config.keep_snapshot {
        if let Err(error) = snapshot::remove(path).await {
            tracing::warn!(%error, "cannot remove the brightness snapshot");
        }
    }

    events
        .send(Event::Brightness(BrightnessEvent::Restored))
        .await
        .map_err(|_| ())?;
    emit_snapshot(events, state).await
}

fn find_entry(
    state: &BrightnessState,
    entry: &crate::brightness::model::BrightnessSnapshotEntry,
) -> Option<usize> {
    if let Some(index) = state
        .devices
        .iter()
        .position(|device| entry.id == device.device.id.stable_id())
    {
        return Some(index);
    }
    if let Some(index) = state
        .devices
        .iter()
        .position(|device| entry.connector.is_some() && entry.connector == device.device.connector)
    {
        return Some(index);
    }
    state
        .devices
        .iter()
        .position(|device| entry.bus.is_some() && entry.bus == device.device.ddc_bus)
}

async fn emit_snapshot(events: &Sender<Event>, state: &BrightnessState) -> Result<(), ()> {
    let devices = state.models();
    events
        .send(Event::Brightness(BrightnessEvent::Snapshot(
            devices.clone(),
        )))
        .await
        .map_err(|_| ())?;
    events
        .send(Event::Snapshot(Snapshot {
            brightness: Some(devices),
            ..Snapshot::default()
        }))
        .await
        .map_err(|_| ())?;
    events
        .send(Event::ServiceAvailability(
            Service::Brightness,
            state.has_available(),
        ))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::brightness::backend::test_support::{fake_candidate, FakeBrightnessBackend};
    use crate::brightness::model::BrightnessSnapshotEntry;

    fn info(name: &str) -> BrightnessDeviceInfo {
        BrightnessDeviceInfo::internal(name)
    }

    fn config() -> BrightnessServiceConfig {
        BrightnessServiceConfig {
            debounce: Duration::from_millis(10),
            poll_interval: Duration::from_millis(50),
            ddc_timeout: Duration::from_millis(100),
            command_timeout: Duration::from_millis(100),
            ..BrightnessServiceConfig::default()
        }
    }

    async fn wait_for<F>(events: &Receiver<Event>, mut test: F) -> Event
    where
        F: FnMut(&Event) -> bool,
    {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("timed out waiting for a brightness event")
                .expect("the event channel closed");
            if test(&event) {
                return event;
            }
        }
    }

    fn service(
        backend: Arc<FakeBrightnessBackend>,
        config: BrightnessServiceConfig,
    ) -> BrightnessService {
        BrightnessService::with_candidates(
            vec![fake_candidate(backend)],
            Vec::new(),
            Vec::new(),
            config,
        )
    }

    #[tokio::test]
    async fn startup_reports_the_internal_device() {
        let backend =
            FakeBrightnessBackend::new("sysfs-logind", vec![info("intel_backlight")]).arc();
        backend
            .set_value(
                &BrightnessDeviceId::Backlight("intel_backlight".into()),
                40.0,
            )
            .await;

        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend, config()).run(command_rx, event_tx));

        let event = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;
        match event {
            Event::Brightness(BrightnessEvent::Snapshot(devices)) => {
                assert_eq!(devices.len(), 1);
                assert_eq!(devices[0].percent, 40.0);
                assert!(devices[0].available);
            }
            other => panic!("unexpected event {other:?}"),
        }

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn rapid_sets_merge_to_the_latest_value() {
        let backend =
            FakeBrightnessBackend::new("sysfs-logind", vec![info("intel_backlight")]).arc();
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend.clone(), config()).run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        for value in [30.0, 40.0, 55.0] {
            commands.send(Command::BrightnessSet(value)).await.unwrap();
        }

        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged {
                    origin: ChangeOrigin::User,
                    ..
                })
            )
        })
        .await;

        let writes = backend.writes().await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1, 55.0);

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn two_rapid_steps_move_twice() {
        let backend =
            FakeBrightnessBackend::new("sysfs-logind", vec![info("intel_backlight")]).arc();
        backend
            .set_value(
                &BrightnessDeviceId::Backlight("intel_backlight".into()),
                40.0,
            )
            .await;

        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend.clone(), config()).run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands.send(Command::BrightnessStep(5.0)).await.unwrap();
        commands.send(Command::BrightnessStep(5.0)).await.unwrap();

        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged {
                    origin: ChangeOrigin::User,
                    ..
                })
            )
        })
        .await;

        let writes = backend.writes().await;
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1, 50.0);

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn values_clamp_to_zero_and_one_hundred() {
        let backend =
            FakeBrightnessBackend::new("sysfs-logind", vec![info("intel_backlight")]).arc();
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend.clone(), config()).run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands.send(Command::BrightnessSet(-5.0)).await.unwrap();
        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged { percent: 0.0, .. })
            )
        })
        .await;

        commands.send(Command::BrightnessSet(500.0)).await.unwrap();
        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged { percent: 100.0, .. })
            )
        })
        .await;

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_failing_device_does_not_stop_the_other_one() {
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        // Two command devices show that the first-device rule applies only to
        // internal panels.
        let device_a = BrightnessDeviceInfo::command("Desk lamp");
        let device_b = BrightnessDeviceInfo::command("Wall light");
        let backend = FakeBrightnessBackend::new("command", vec![device_a, device_b]).arc();
        backend
            .failing
            .lock()
            .await
            .insert(BrightnessDeviceId::Named("Desk lamp".into()));

        let service = BrightnessService::with_candidates(
            Vec::new(),
            Vec::new(),
            vec![fake_candidate(backend.clone())],
            config(),
        );
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands.send(Command::BrightnessSet(20.0)).await.unwrap();

        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged {
                    origin: ChangeOrigin::User,
                    ..
                })
            )
        })
        .await;

        let writes = backend.writes().await;
        assert!(writes.iter().any(|(id, value)| id
            == &BrightnessDeviceId::Named("Wall light".into())
            && *value == 20.0));
        assert!(writes
            .iter()
            .any(|(id, _)| id == &BrightnessDeviceId::Named("Desk lamp".into())));

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn a_ddc_write_failure_keeps_the_device_when_the_read_succeeds() {
        let id = BrightnessDeviceId::Ddc {
            bus: 7,
            display: "display-7".into(),
        };
        let info = BrightnessDeviceInfo {
            id: id.clone(),
            kind: BrightnessKind::ExternalDdc,
            name: "display-7".into(),
            connector: None,
            ddc_bus: Some(7),
        };
        let backend = FakeBrightnessBackend::new("ddcutil", vec![info]).arc();
        backend.set_value(&id, 42.0).await;
        backend.failing.lock().await.insert(id.clone());

        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let service = BrightnessService::with_candidates(
            Vec::new(),
            vec![fake_candidate(backend)],
            Vec::new(),
            config(),
        );
        let task = tokio::spawn(service.run(command_rx, event_tx));

        wait_for(&event_rx, |event| match event {
            Event::Brightness(BrightnessEvent::Snapshot(devices)) => {
                devices.iter().any(|device| device.id == id)
            }
            _ => false,
        })
        .await;

        commands.send(Command::BrightnessSet(20.0)).await.unwrap();

        wait_for(&event_rx, |event| match event {
            Event::Brightness(BrightnessEvent::Snapshot(devices)) => devices
                .iter()
                .any(|device| device.id == id && device.available && device.percent == 42.0),
            _ => false,
        })
        .await;

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn dim_snapshots_then_dims_every_device() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let mut config = config();
        config.snapshot_path = Some(path.clone());

        let info_a = BrightnessDeviceInfo::command("Desk lamp");
        let info_b = BrightnessDeviceInfo::command("Wall light");
        let backend = FakeBrightnessBackend::new("command", vec![info_a, info_b]).arc();
        backend
            .set_value(&BrightnessDeviceId::Named("Desk lamp".into()), 70.0)
            .await;
        backend
            .set_value(&BrightnessDeviceId::Named("Wall light".into()), 60.0)
            .await;

        let service = BrightnessService::with_candidates(
            Vec::new(),
            Vec::new(),
            vec![fake_candidate(backend.clone())],
            config,
        );
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands
            .send(Command::BrightnessDim { value: 10.0 })
            .await
            .unwrap();
        wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged {
                    origin: ChangeOrigin::Automatic,
                    percent: 10.0,
                    ..
                })
            )
        })
        .await;

        let snapshot = snapshot::load(&path).await.unwrap().unwrap();
        assert_eq!(snapshot.devices.len(), 2);
        assert!(snapshot
            .devices
            .iter()
            .all(|entry| entry.percent == 70.0 || entry.percent == 60.0));

        let writes = backend.writes().await;
        assert!(writes.iter().any(|(_, value)| *value == 10.0));

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn restore_applies_entries_and_removes_the_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let mut config = config();
        config.snapshot_path = Some(path.clone());

        let id = BrightnessDeviceId::Named("Desk lamp".into());
        let file = BrightnessSnapshotFile {
            created: 1,
            devices: vec![BrightnessSnapshotEntry {
                id: id.stable_id(),
                kind: "command".to_string(),
                percent: 65.0,
                raw: None,
                bus: None,
                connector: None,
                value: None,
            }],
        };
        snapshot::save(&path, &file).await.unwrap();

        let backend =
            FakeBrightnessBackend::new("command", vec![BrightnessDeviceInfo::command("Desk lamp")])
                .arc();
        let service = BrightnessService::with_candidates(
            Vec::new(),
            Vec::new(),
            vec![fake_candidate(backend.clone())],
            config,
        );
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands.send(Command::BrightnessRestore).await.unwrap();
        wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Restored))
        })
        .await;

        let writes = backend.writes().await;
        assert!(writes
            .iter()
            .any(|(written, value)| written == &id && *value == 65.0));
        assert_eq!(snapshot::load(&path).await.unwrap(), None);

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn restore_keeps_the_snapshot_when_a_device_is_missing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        let mut config = config();
        config.snapshot_path = Some(path.clone());

        let file = BrightnessSnapshotFile {
            created: 1,
            devices: vec![
                BrightnessSnapshotEntry {
                    id: "command:Desk lamp".to_string(),
                    kind: "command".to_string(),
                    percent: 65.0,
                    raw: None,
                    bus: None,
                    connector: None,
                    value: None,
                },
                BrightnessSnapshotEntry {
                    id: "command:Missing lamp".to_string(),
                    kind: "command".to_string(),
                    percent: 30.0,
                    raw: None,
                    bus: None,
                    connector: None,
                    value: None,
                },
            ],
        };
        snapshot::save(&path, &file).await.unwrap();

        let backend =
            FakeBrightnessBackend::new("command", vec![BrightnessDeviceInfo::command("Desk lamp")])
                .arc();
        let service = BrightnessService::with_candidates(
            Vec::new(),
            Vec::new(),
            vec![fake_candidate(backend)],
            config,
        );
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service.run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        commands.send(Command::BrightnessRestore).await.unwrap();
        wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Restored))
        })
        .await;

        assert!(snapshot::load(&path).await.unwrap().is_some());

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn an_external_file_change_uses_the_external_origin() {
        let backend =
            FakeBrightnessBackend::new("sysfs-logind", vec![info("intel_backlight")]).arc();
        let id = BrightnessDeviceId::Backlight("intel_backlight".into());
        backend.set_value(&id, 40.0).await;

        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend.clone(), config()).run(command_rx, event_tx));

        let _ = wait_for(&event_rx, |event| {
            matches!(event, Event::Brightness(BrightnessEvent::Snapshot(_)))
        })
        .await;

        backend.set_value(&id, 70.0).await;

        let event = wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::Brightness(BrightnessEvent::DeviceChanged {
                    origin: ChangeOrigin::External,
                    ..
                })
            )
        })
        .await;
        match event {
            Event::Brightness(BrightnessEvent::DeviceChanged { percent, .. }) => {
                assert_eq!(percent, 70.0);
            }
            other => panic!("unexpected event {other:?}"),
        }

        drop(commands);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn no_devices_reports_unavailable() {
        let backend = FakeBrightnessBackend::new("sysfs-logind", Vec::new()).arc();
        let (commands, command_rx) = async_channel::unbounded();
        let (event_tx, event_rx) = async_channel::unbounded();
        let task = tokio::spawn(service(backend, config()).run(command_rx, event_tx));

        let event = wait_for(&event_rx, |event| {
            matches!(
                event,
                Event::ServiceAvailability(Service::Brightness, false)
            )
        })
        .await;
        assert!(matches!(
            event,
            Event::ServiceAvailability(Service::Brightness, false)
        ));

        drop(commands);
        task.await.unwrap();
    }
}
