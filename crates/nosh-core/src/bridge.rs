use std::thread;

use async_channel::{Receiver, Sender};

use crate::audio::model::{volume_icon, AudioEndpoint, AudioModel, EndpointKind};
use crate::audio::service::{AudioService, AudioServiceConfig};
use crate::battery::service::{BatteryService, BatteryServiceConfig};
use crate::brightness::service::{BrightnessService, BrightnessServiceConfig};
use crate::command::Command;
use crate::error::CoreError;
use crate::event::{
    AudioEvent, BatteryEvent, BrightnessEvent, Event, IdleInhibitEvent, MonitorEvent, NetworkEvent,
    NotificationEvent, PowerProfilesEvent, Snapshot, SystemStatsEvent, TrayEvent, WorkspaceEvent,
};
use crate::hyprland::{binds, Hyprland};
use crate::idle_inhibit::model::IdleInhibitState;
use crate::idle_inhibit::service::{IdleInhibitService, IdleInhibitServiceConfig};
use crate::menu::service::{MenuService, MenuServiceConfig};
use crate::monitors::service::{MonitorService, MonitorServiceConfig};
use crate::network::service::{NetworkService, NetworkServiceConfig};
use crate::notifications::service::{NotificationService, NotificationServiceConfig};
use crate::power_profiles::service::{PowerProfilesService, PowerProfilesServiceConfig};
use crate::stats::service::{SystemStatsService, SystemStatsServiceConfig};
use crate::tray::service::{TrayService, TrayServiceConfig};
use crate::workspaces::service::{WorkspaceService, WorkspaceServiceConfig};

#[derive(Debug, Clone, Default)]
pub struct CoreSettings {
    pub workspaces: WorkspaceServiceConfig,
    pub monitors: MonitorServiceConfig,
    pub audio: AudioServiceConfig,
    pub brightness: BrightnessServiceConfig,
    pub battery: BatteryServiceConfig,
    pub stats: SystemStatsServiceConfig,
    pub idle_inhibit: IdleInhibitServiceConfig,
    pub power_profiles: PowerProfilesServiceConfig,
    pub network: NetworkServiceConfig,
    pub tray: TrayServiceConfig,
    pub menu: MenuServiceConfig,
    pub notifications: NotificationServiceConfig,
}

/// Handle to the core thread.
///
/// The core runs on its own Tokio runtime. The UI subscribes to events before
/// creating windows, sends commands, and never blocks waiting for a reply.
pub struct Core {
    commands: Sender<Command>,
    events: Receiver<Event>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Core {
    pub fn start() -> Result<Self, CoreError> {
        let mut settings = CoreSettings::default();

        match crate::paths::NoshPaths::from_env() {
            Ok(paths) => {
                match crate::menu::load_menu_config(&paths.config_dir) {
                    Ok(config) => settings.menu.menus = config.menus,
                    Err(error) => tracing::error!(%error, "the menu config could not be loaded"),
                }
                settings.menu.paths = Some(paths.clone());
                settings.notifications.paths = Some(paths);
            }
            Err(error) => tracing::error!(%error, "the config paths could not be resolved"),
        }

        Self::start_with(settings)
    }

    pub fn start_with(settings: CoreSettings) -> Result<Self, CoreError> {
        let menu_map = settings.menu.menus.clone();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nosh-core")
            .build()?;

        let (commands, public_commands) = async_channel::unbounded();
        let (event_sender, events) = async_channel::unbounded();
        let (service_events, service_event_receiver) = async_channel::unbounded();
        let (workspace_commands, workspace_receiver) = async_channel::unbounded();
        let (monitor_commands, monitor_receiver) = async_channel::unbounded();
        let (audio_commands, audio_receiver) = async_channel::unbounded();
        let (brightness_commands, brightness_receiver) = async_channel::unbounded();
        let (battery_commands, battery_receiver) = async_channel::unbounded();
        let (stats_commands, stats_receiver) = async_channel::unbounded();
        let (idle_inhibit_commands, idle_inhibit_receiver) = async_channel::unbounded();
        let (power_profiles_commands, power_profiles_receiver) = async_channel::unbounded();
        let (network_commands, network_receiver) = async_channel::unbounded();
        let (tray_commands, tray_receiver) = async_channel::unbounded();
        let (menu_commands, menu_receiver) = async_channel::unbounded();
        let (notification_commands, notification_receiver) = async_channel::unbounded();

        let thread = thread::Builder::new()
            .name("nosh-core".to_string())
            .spawn(move || {
                runtime.block_on(async move {
                    let router = tokio::spawn(route_commands(
                        public_commands,
                        ServiceSenders {
                            workspaces: workspace_commands,
                            monitors: monitor_commands,
                            audio: audio_commands,
                            brightness: brightness_commands,
                            _battery: battery_commands,
                            _stats: stats_commands,
                            idle_inhibit: idle_inhibit_commands,
                            power_profiles: power_profiles_commands,
                            network: network_commands,
                            tray: tray_commands,
                            menu: menu_commands,
                            notifications: notification_commands,
                        },
                    ));
                    let merger = tokio::spawn(merge_events(service_event_receiver, event_sender));

                    let bind_sync = Hyprland::detect()
                        .ok()
                        .flatten()
                        .map(|hypr| tokio::spawn(binds::run(hypr, menu_map)));

                    let workspaces = WorkspaceService::from_env_with(settings.workspaces)
                        .run(workspace_receiver, service_events.clone());
                    let monitors = MonitorService::from_env_with(settings.monitors)
                        .run(monitor_receiver, service_events.clone());
                    let audio = AudioService::from_env_with(settings.audio)
                        .run(audio_receiver, service_events.clone());
                    let brightness = BrightnessService::from_env_with(settings.brightness)
                        .run(brightness_receiver, service_events.clone());
                    let battery = BatteryService::from_env_with(settings.battery)
                        .run(battery_receiver, service_events.clone());
                    let stats = SystemStatsService::from_env_with(settings.stats)
                        .run(stats_receiver, service_events.clone());
                    let idle_inhibit = IdleInhibitService::from_env_with(settings.idle_inhibit)
                        .run(idle_inhibit_receiver, service_events.clone());
                    let power_profiles =
                        PowerProfilesService::from_env_with(settings.power_profiles)
                            .run(power_profiles_receiver, service_events.clone());
                    let network = NetworkService::from_env_with(settings.network)
                        .run(network_receiver, service_events.clone());
                    let tray = TrayService::from_env_with(settings.tray)
                        .run(tray_receiver, service_events.clone());
                    let menu =
                        MenuService::new(settings.menu).run(menu_receiver, service_events.clone());
                    let notifications = NotificationService::from_env_with(settings.notifications)
                        .run(notification_receiver, service_events.clone());
                    // The services own a clone. Dropping the original lets the
                    // merger finish once every service returns.
                    drop(service_events);

                    tokio::join!(
                        workspaces,
                        monitors,
                        audio,
                        brightness,
                        battery,
                        stats,
                        idle_inhibit,
                        power_profiles,
                        network,
                        tray,
                        menu,
                        notifications
                    );
                    router.abort();
                    if let Some(bind_sync) = bind_sync {
                        bind_sync.abort();
                    }
                    let _ = merger.await;
                });
            })?;

        Ok(Self {
            commands,
            events,
            thread: Some(thread),
        })
    }

    pub fn subscribe(&self) -> Receiver<Event> {
        self.events.clone()
    }

    /// Queues a command without blocking. The core channel is unbounded, so a
    /// failure means the core has stopped.
    pub fn send(&self, command: Command) {
        if let Err(error) = self.commands.try_send(command) {
            tracing::warn!(%error, "core command was not delivered");
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.commands.close();
        self.events.close();

        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::warn!("the core thread panicked");
            }
        }
    }
}

/// Routes each public command to the service that handles it.
///
/// Each service needs exclusive access to its command receiver, so the public
/// channel fans out to one service channel per service. The battery and stats
/// services take no commands; their senders are held here so that their
/// receivers close when the public channel closes.
struct ServiceSenders {
    workspaces: Sender<Command>,
    monitors: Sender<Command>,
    audio: Sender<Command>,
    brightness: Sender<Command>,
    _battery: Sender<Command>,
    _stats: Sender<Command>,
    idle_inhibit: Sender<Command>,
    power_profiles: Sender<Command>,
    network: Sender<Command>,
    tray: Sender<Command>,
    menu: Sender<Command>,
    notifications: Sender<Command>,
}

async fn route_commands(commands: Receiver<Command>, senders: ServiceSenders) {
    while let Ok(command) = commands.recv().await {
        let sender = if command.is_audio() {
            &senders.audio
        } else if command.is_brightness() {
            &senders.brightness
        } else if command.is_monitor() {
            &senders.monitors
        } else if command.is_idle_inhibit() {
            &senders.idle_inhibit
        } else if command.is_power_profiles() {
            &senders.power_profiles
        } else if command.is_network() {
            &senders.network
        } else if command.is_tray() {
            &senders.tray
        } else if command.is_menu() {
            &senders.menu
        } else if command.is_notifications() {
            &senders.notifications
        } else {
            &senders.workspaces
        };

        if sender.send(command).await.is_err() {
            tracing::debug!("a service command channel is closed");
        }
    }
}

/// Merges partial snapshots from the services into one accumulator and forwards
/// every event in order.
///
/// Typed updates refresh the accumulator, so the next full snapshot carries the
/// latest state. A snapshot field is replaced only when the incoming value is
/// `Some`, so a partial snapshot never erases the state of another service.
async fn merge_events(events: Receiver<Event>, public: Sender<Event>) {
    let mut snapshot = Snapshot::default();

    while let Ok(event) = events.recv().await {
        let event = match event {
            Event::Snapshot(partial) => {
                merge_snapshot(&mut snapshot, partial);
                Event::Snapshot(snapshot.clone())
            }
            event => {
                absorb_event(&mut snapshot, &event);
                event
            }
        };

        if public.send(event).await.is_err() {
            return;
        }
    }
}

fn merge_snapshot(accumulator: &mut Snapshot, incoming: Snapshot) {
    if incoming.workspaces.is_some() {
        accumulator.workspaces = incoming.workspaces;
    }
    if incoming.workspace_backend.is_some() {
        accumulator.workspace_backend = incoming.workspace_backend;
    }
    if incoming.monitors.is_some() {
        accumulator.monitors = incoming.monitors;
    }
    if incoming.monitor_backend.is_some() {
        accumulator.monitor_backend = incoming.monitor_backend;
    }
    if incoming.audio.is_some() {
        accumulator.audio = incoming.audio;
    }
    if incoming.audio_backend.is_some() {
        accumulator.audio_backend = incoming.audio_backend;
    }
    if incoming.brightness.is_some() {
        accumulator.brightness = incoming.brightness;
    }
    if incoming.battery.is_some() {
        accumulator.battery = incoming.battery;
    }
    if incoming.system_stats.is_some() {
        accumulator.system_stats = incoming.system_stats;
    }
    if incoming.idle_inhibit.is_some() {
        accumulator.idle_inhibit = incoming.idle_inhibit;
    }
    if incoming.idle_inhibit_backend.is_some() {
        accumulator.idle_inhibit_backend = incoming.idle_inhibit_backend;
    }
    if incoming.power_profiles.is_some() {
        accumulator.power_profiles = incoming.power_profiles;
    }
    if incoming.network.is_some() {
        accumulator.network = incoming.network;
    }
    if incoming.network_backend.is_some() {
        accumulator.network_backend = incoming.network_backend;
    }
    if incoming.tray.is_some() {
        accumulator.tray = incoming.tray;
    }
    if incoming.tray_backend.is_some() {
        accumulator.tray_backend = incoming.tray_backend;
    }
    if incoming.notifications.is_some() {
        accumulator.notifications = incoming.notifications;
    }
    if incoming.notification_backend.is_some() {
        accumulator.notification_backend = incoming.notification_backend;
    }
}

fn absorb_event(accumulator: &mut Snapshot, event: &Event) {
    match event {
        Event::Workspaces(WorkspaceEvent::Snapshot { state, backend }) => {
            accumulator.workspaces = Some(state.clone());
            accumulator.workspace_backend = Some(*backend);
        }
        Event::Monitors(MonitorEvent::Snapshot { layout, backend }) => {
            accumulator.monitors = Some(layout.clone());
            accumulator.monitor_backend = Some(*backend);
        }
        Event::Monitors(MonitorEvent::LayoutChanged(layout)) => {
            accumulator.monitors = Some(layout.clone());
        }
        Event::Audio(event) => absorb_audio(accumulator, event),
        Event::Brightness(event) => absorb_brightness(accumulator, event),
        Event::Battery(event) => absorb_battery(accumulator, event),
        Event::SystemStats(event) => absorb_system_stats(accumulator, event),
        Event::IdleInhibit(event) => absorb_idle_inhibit(accumulator, event),
        Event::PowerProfiles(event) => absorb_power_profiles(accumulator, event),
        Event::Network(event) => absorb_network(accumulator, event),
        Event::Tray(event) => absorb_tray(accumulator, event),
        Event::Notifications(event) => absorb_notifications(accumulator, event),
        _ => {}
    }
}

fn absorb_battery(accumulator: &mut Snapshot, event: &BatteryEvent) {
    match event {
        BatteryEvent::Snapshot(model) | BatteryEvent::Changed(model) => {
            accumulator.battery = Some(model.clone());
        }
        BatteryEvent::Unavailable => accumulator.battery = None,
    }
}

fn absorb_system_stats(accumulator: &mut Snapshot, event: &SystemStatsEvent) {
    match event {
        SystemStatsEvent::Changed(stats) => accumulator.system_stats = Some(*stats),
    }
}

fn absorb_idle_inhibit(accumulator: &mut Snapshot, event: &IdleInhibitEvent) {
    match event {
        IdleInhibitEvent::Snapshot { available, active } => {
            accumulator.idle_inhibit = Some(IdleInhibitState::new(*available, *active));
        }
        IdleInhibitEvent::Changed { active } => {
            if let Some(state) = accumulator.idle_inhibit.as_mut() {
                state.active = *active;
            }
        }
        IdleInhibitEvent::Unavailable => accumulator.idle_inhibit = None,
    }
}

fn absorb_power_profiles(accumulator: &mut Snapshot, event: &PowerProfilesEvent) {
    match event {
        PowerProfilesEvent::Snapshot(model) | PowerProfilesEvent::Changed(model) => {
            accumulator.power_profiles = Some(model.clone());
        }
        PowerProfilesEvent::Unavailable => accumulator.power_profiles = None,
    }
}

fn absorb_network(accumulator: &mut Snapshot, event: &NetworkEvent) {
    match event {
        NetworkEvent::Snapshot(model) | NetworkEvent::Changed(model) => {
            accumulator.network = Some(model.clone());
            accumulator.network_backend = Some("networkmanager");
        }
        NetworkEvent::Unavailable => {
            accumulator.network = None;
            accumulator.network_backend = None;
        }
        NetworkEvent::ScanStarted
        | NetworkEvent::ScanFinished
        | NetworkEvent::OperationFailed(_) => {}
    }
}

fn absorb_tray(accumulator: &mut Snapshot, event: &TrayEvent) {
    match event {
        TrayEvent::Snapshot(items) => accumulator.tray = Some(items.clone()),
        TrayEvent::ItemAdded { item, .. } | TrayEvent::ItemUpdated { item } => {
            let Some(items) = accumulator.tray.as_mut() else {
                return;
            };
            if let Some(existing) = items.iter_mut().find(|entry| entry.item_id == item.item_id) {
                *existing = item.clone();
            } else {
                items.push(item.clone());
            }
        }
        TrayEvent::ItemRemoved { item_id } => {
            if let Some(items) = accumulator.tray.as_mut() {
                items.retain(|item| &item.item_id != item_id);
            }
        }
        TrayEvent::MenuUpdated { .. } => {}
        TrayEvent::BackendChanged { backend } => {
            accumulator.tray_backend = Some(*backend);
        }
        TrayEvent::Unavailable => accumulator.tray = None,
    }
}

fn absorb_notifications(accumulator: &mut Snapshot, event: &NotificationEvent) {
    match event {
        NotificationEvent::Snapshot(model) => {
            accumulator.notifications = Some(model.clone());
        }
        NotificationEvent::Received { notification, .. } => {
            if let Some(model) = accumulator.notifications.as_mut() {
                model.upsert(notification.clone());
            }
        }
        NotificationEvent::Closed { id, .. } => {
            if let Some(model) = accumulator.notifications.as_mut() {
                model.remove(*id);
            }
        }
        NotificationEvent::DndChanged { enabled } => {
            if let Some(model) = accumulator.notifications.as_mut() {
                model.settings.dont_disturb = *enabled;
            }
        }
        NotificationEvent::BackendChanged { backend } => {
            accumulator.notification_backend = Some(*backend);
        }
        NotificationEvent::Invoked { .. } => {}
        NotificationEvent::Unavailable => {
            accumulator.notifications = None;
            accumulator.notification_backend = None;
        }
    }
}

fn absorb_brightness(accumulator: &mut Snapshot, event: &BrightnessEvent) {
    match event {
        BrightnessEvent::Snapshot(devices) => accumulator.brightness = Some(devices.clone()),
        BrightnessEvent::DeviceChanged {
            device, percent, ..
        } => {
            let Some(devices) = accumulator.brightness.as_mut() else {
                return;
            };
            if let Some(entry) = devices.iter_mut().find(|entry| &entry.id == device) {
                entry.percent = *percent;
                entry.pending_percent = None;
                entry.available = true;
            }
        }
        BrightnessEvent::Pending { device, target } => {
            let Some(devices) = accumulator.brightness.as_mut() else {
                return;
            };
            if let Some(entry) = devices.iter_mut().find(|entry| &entry.id == device) {
                entry.pending_percent = Some(*target);
            }
        }
        BrightnessEvent::Restored | BrightnessEvent::Unavailable => {}
    }
}

fn absorb_audio(accumulator: &mut Snapshot, event: &AudioEvent) {
    match event {
        AudioEvent::Snapshot(model) => accumulator.audio = Some(model.clone()),
        AudioEvent::VolumeChanged {
            kind, id, volume, ..
        } => {
            let Some(model) = accumulator.audio.as_mut() else {
                return;
            };
            let Some(updated) = update_endpoint(model, *kind, *id, |endpoint| {
                endpoint.volume = *volume;
                endpoint.icon_name =
                    volume_icon(*kind, endpoint.volume, endpoint.muted).to_string();
            }) else {
                return;
            };
            if model
                .default_for(*kind)
                .is_some_and(|default| default.id == *id)
            {
                if let Some(default) = model.default_for_mut(*kind) {
                    *default = updated;
                }
            }
        }
        AudioEvent::MuteChanged {
            kind, id, muted, ..
        } => {
            let Some(model) = accumulator.audio.as_mut() else {
                return;
            };
            let Some(updated) = update_endpoint(model, *kind, *id, |endpoint| {
                endpoint.muted = *muted;
                endpoint.icon_name =
                    volume_icon(*kind, endpoint.volume, endpoint.muted).to_string();
            }) else {
                return;
            };
            if model
                .default_for(*kind)
                .is_some_and(|default| default.id == *id)
            {
                if let Some(default) = model.default_for_mut(*kind) {
                    *default = updated;
                }
            }
        }
        AudioEvent::DefaultChanged { kind, endpoint } => {
            let Some(model) = accumulator.audio.as_mut() else {
                return;
            };
            match kind {
                EndpointKind::Sink => {
                    model.default_sink = Some(endpoint.clone());
                    for sink in &mut model.sinks {
                        sink.is_default = sink.id == endpoint.id;
                    }
                }
                EndpointKind::Source => {
                    model.default_source = Some(endpoint.clone());
                    for source in &mut model.sources {
                        source.is_default = source.id == endpoint.id;
                    }
                }
            }
        }
        AudioEvent::BackendChanged { backend } => {
            accumulator.audio_backend = Some(*backend);
        }
        AudioEvent::WriteFailed(_) | AudioEvent::ReadFailed(_) | AudioEvent::Unavailable => {}
    }
}

fn update_endpoint(
    model: &mut AudioModel,
    kind: EndpointKind,
    id: u32,
    update: impl FnOnce(&mut AudioEndpoint),
) -> Option<AudioEndpoint> {
    let endpoint = model.endpoint_mut(kind, id)?;
    update(endpoint);
    Some(endpoint.clone())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::event::Service;
    use crate::monitors::LayoutPreset;
    use crate::monitors::MonitorLayout;
    use crate::workspaces::model::WorkspaceSnapshot;

    #[tokio::test]
    async fn commands_are_routed_to_the_matching_service() {
        let (public_tx, public_rx) = async_channel::unbounded();
        let (workspace_tx, workspace_rx) = async_channel::unbounded();
        let (monitor_tx, monitor_rx) = async_channel::unbounded();
        let (audio_tx, audio_rx) = async_channel::unbounded();
        let (brightness_tx, brightness_rx) = async_channel::unbounded();
        let (battery_tx, _battery_rx) = async_channel::unbounded();
        let (stats_tx, _stats_rx) = async_channel::unbounded();
        let (idle_inhibit_tx, idle_inhibit_rx) = async_channel::unbounded();
        let (power_profiles_tx, power_profiles_rx) = async_channel::unbounded();
        let (network_tx, network_rx) = async_channel::unbounded();
        let (tray_tx, tray_rx) = async_channel::unbounded();
        let (menu_tx, _menu_rx) = async_channel::unbounded();
        let (notification_tx, _notification_rx) = async_channel::unbounded();

        let task = tokio::spawn(route_commands(
            public_rx,
            ServiceSenders {
                workspaces: workspace_tx,
                monitors: monitor_tx,
                audio: audio_tx,
                brightness: brightness_tx,
                _battery: battery_tx,
                _stats: stats_tx,
                idle_inhibit: idle_inhibit_tx,
                power_profiles: power_profiles_tx,
                network: network_tx,
                tray: tray_tx,
                menu: menu_tx,
                notifications: notification_tx,
            },
        ));

        public_tx
            .send(Command::WorkspaceActivate("1".to_string()))
            .await
            .unwrap();
        public_tx
            .send(Command::MonitorSetPrimary("DP-1".to_string()))
            .await
            .unwrap();
        public_tx
            .send(Command::MonitorMode {
                target: "1".to_string(),
                mode: LayoutPreset::Extend,
            })
            .await
            .unwrap();
        public_tx.send(Command::AudioSetVolume(0.5)).await.unwrap();
        public_tx.send(Command::BrightnessSet(40.0)).await.unwrap();
        public_tx.send(Command::IdleInhibitToggle).await.unwrap();
        public_tx.send(Command::PowerProfileCycle).await.unwrap();
        public_tx
            .send(Command::AirplaneModeSet(true))
            .await
            .unwrap();
        public_tx
            .send(Command::TrayMenuAboutToShow {
                item_id: "item".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), workspace_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::WorkspaceActivate("1".to_string())
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), monitor_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::MonitorSetPrimary("DP-1".to_string())
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), monitor_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::MonitorMode {
                target: "1".to_string(),
                mode: LayoutPreset::Extend,
            }
        );
        assert!(workspace_rx.try_recv().is_err());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), audio_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::AudioSetVolume(0.5)
        );
        assert!(monitor_rx.try_recv().is_err());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), brightness_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::BrightnessSet(40.0)
        );
        assert!(audio_rx.try_recv().is_err());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), idle_inhibit_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::IdleInhibitToggle
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), power_profiles_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::PowerProfileCycle
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), network_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::AirplaneModeSet(true)
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), tray_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::TrayMenuAboutToShow {
                item_id: "item".to_string()
            }
        );

        drop(public_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn two_partial_snapshots_merge_into_one_full_snapshot() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        let workspaces = WorkspaceSnapshot {
            focused_workspace: Some("1".to_string()),
            ..WorkspaceSnapshot::default()
        };
        service_tx
            .send(Event::Snapshot(Snapshot {
                workspaces: Some(workspaces),
                workspace_backend: Some("hyprland"),
                ..Snapshot::default()
            }))
            .await
            .unwrap();

        let first = public_rx.recv().await.unwrap();
        match first {
            Event::Snapshot(snapshot) => {
                assert!(snapshot.workspaces.is_some());
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert!(snapshot.monitors.is_none());
                assert!(snapshot.monitor_backend.is_none());
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        let monitors = MonitorLayout {
            primary: Some("DP-1".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Snapshot(Snapshot {
                monitors: Some(monitors),
                monitor_backend: Some("hyprland"),
                ..Snapshot::default()
            }))
            .await
            .unwrap();

        let second = public_rx.recv().await.unwrap();
        match second {
            Event::Snapshot(snapshot) => {
                assert_eq!(
                    snapshot
                        .workspaces
                        .expect("workspace state was not merged")
                        .focused_workspace
                        .as_deref(),
                    Some("1")
                );
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert_eq!(
                    snapshot
                        .monitors
                        .expect("monitor state was not merged")
                        .primary
                        .as_deref(),
                    Some("DP-1")
                );
                assert_eq!(snapshot.monitor_backend, Some("hyprland"));
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn typed_updates_are_reflected_in_the_next_full_snapshot() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        let workspaces = WorkspaceSnapshot {
            focused_workspace: Some("3".to_string()),
            ..WorkspaceSnapshot::default()
        };
        service_tx
            .send(Event::Workspaces(WorkspaceEvent::Snapshot {
                state: workspaces,
                backend: "hyprland",
            }))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Workspaces(WorkspaceEvent::Snapshot { .. })
        ));

        let first_monitor_layout = MonitorLayout {
            primary: Some("DP-1".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Monitors(MonitorEvent::Snapshot {
                layout: first_monitor_layout,
                backend: "hyprland",
            }))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::Snapshot { .. })
        ));

        let updated_monitor_layout = MonitorLayout {
            primary: Some("DP-2".to_string()),
            ..MonitorLayout::default()
        };
        service_tx
            .send(Event::Monitors(MonitorEvent::LayoutChanged(
                updated_monitor_layout,
            )))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::LayoutChanged(_))
        ));

        service_tx
            .send(Event::Snapshot(Snapshot::default()))
            .await
            .unwrap();
        let event = public_rx.recv().await.unwrap();
        match event {
            Event::Snapshot(snapshot) => {
                assert_eq!(
                    snapshot
                        .workspaces
                        .expect("workspace update was not absorbed")
                        .focused_workspace
                        .as_deref(),
                    Some("3")
                );
                assert_eq!(snapshot.workspace_backend, Some("hyprland"));
                assert_eq!(
                    snapshot
                        .monitors
                        .expect("monitor update was not absorbed")
                        .primary
                        .as_deref(),
                    Some("DP-2")
                );
                assert_eq!(snapshot.monitor_backend, Some("hyprland"));
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn audio_updates_are_reflected_in_the_next_full_snapshot() {
        use crate::audio::model::{AudioEndpoint, AudioModel, EndpointKind};

        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        let sink = AudioEndpoint {
            id: 1,
            name: "sink".to_string(),
            description: "Sink".to_string(),
            volume: 0.3,
            muted: false,
            icon_name: "audio-volume-low-symbolic".to_string(),
            is_default: true,
        };
        let audio = AudioModel {
            default_sink: Some(sink.clone()),
            sinks: vec![sink],
            ..AudioModel::default()
        };

        service_tx
            .send(Event::Audio(AudioEvent::Snapshot(audio)))
            .await
            .unwrap();
        service_tx
            .send(Event::Audio(AudioEvent::VolumeChanged {
                kind: EndpointKind::Sink,
                id: 1,
                volume: 0.7,
                old: 0.3,
                origin: crate::origin::ChangeOrigin::External,
            }))
            .await
            .unwrap();
        service_tx
            .send(Event::Snapshot(Snapshot::default()))
            .await
            .unwrap();

        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Audio(AudioEvent::Snapshot(_))
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Audio(AudioEvent::VolumeChanged { .. })
        ));
        let event = public_rx.recv().await.unwrap();
        match event {
            Event::Snapshot(snapshot) => {
                let audio = snapshot.audio.expect("audio was not absorbed");
                assert_eq!(audio.sinks[0].volume, 0.7);
                assert_eq!(
                    audio
                        .default_sink
                        .expect("the default sink was lost")
                        .volume,
                    0.7
                );
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn idle_inhibit_updates_are_reflected_in_the_next_full_snapshot() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        service_tx
            .send(Event::IdleInhibit(IdleInhibitEvent::Snapshot {
                available: true,
                active: false,
            }))
            .await
            .unwrap();
        service_tx
            .send(Event::IdleInhibit(IdleInhibitEvent::Changed {
                active: true,
            }))
            .await
            .unwrap();
        service_tx
            .send(Event::Snapshot(Snapshot {
                idle_inhibit_backend: Some("wayland"),
                ..Snapshot::default()
            }))
            .await
            .unwrap();

        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::IdleInhibit(IdleInhibitEvent::Snapshot { .. })
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::IdleInhibit(IdleInhibitEvent::Changed { .. })
        ));
        match public_rx.recv().await.unwrap() {
            Event::Snapshot(snapshot) => {
                let state = snapshot
                    .idle_inhibit
                    .expect("the idle inhibitor state was not absorbed");
                assert!(state.available);
                assert!(state.active);
                assert_eq!(snapshot.idle_inhibit_backend, Some("wayland"));
            }
            other => panic!("expected a merged snapshot, got {other:?}"),
        }

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn event_order_is_preserved() {
        let (service_tx, service_rx) = async_channel::unbounded();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        service_tx
            .send(Event::Workspaces(WorkspaceEvent::FocusedChanged {
                workspace: Some("1".to_string()),
            }))
            .await
            .unwrap();
        service_tx
            .send(Event::Monitors(MonitorEvent::ApplyFailed(
                "rejected".to_string(),
            )))
            .await
            .unwrap();
        service_tx
            .send(Event::ServiceAvailability(Service::Workspaces, true))
            .await
            .unwrap();
        service_tx
            .send(Event::Snapshot(Snapshot::default()))
            .await
            .unwrap();

        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Workspaces(WorkspaceEvent::FocusedChanged { .. })
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Monitors(MonitorEvent::ApplyFailed(_))
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::ServiceAvailability(Service::Workspaces, true)
        ));
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::Snapshot(_)
        ));

        drop(service_tx);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn the_merger_exits_after_both_service_senders_drop() {
        let (first_service, service_rx) = async_channel::unbounded();
        let second_service = first_service.clone();
        let (public_tx, public_rx) = async_channel::unbounded();
        let task = tokio::spawn(merge_events(service_rx, public_tx));

        drop(first_service);
        second_service
            .send(Event::ServiceAvailability(Service::Monitors, true))
            .await
            .unwrap();
        assert!(matches!(
            public_rx.recv().await.unwrap(),
            Event::ServiceAvailability(Service::Monitors, true)
        ));
        drop(second_service);

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
}
