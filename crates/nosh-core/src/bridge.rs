use std::thread;

use async_channel::{Receiver, Sender};

use crate::audio::service::{AudioService, AudioServiceConfig};
use crate::auth::service::{AuthService, AuthServiceConfig};
use crate::battery::service::{BatteryService, BatteryServiceConfig};
use crate::brightness::service::{BrightnessService, BrightnessServiceConfig};
use crate::command::{Command, Service};
use crate::error::CoreError;
use crate::event::Event;
use crate::hyprland::{binds, Hyprland};
use crate::idle_inhibit::service::{IdleInhibitService, IdleInhibitServiceConfig};
use crate::media::service::{MediaService, MediaServiceConfig};
use crate::menu::service::{MenuService, MenuServiceConfig};
use crate::monitors::service::{MonitorService, MonitorServiceConfig};
use crate::network::service::{NetworkService, NetworkServiceConfig};
use crate::notifications::service::{NotificationService, NotificationServiceConfig};
use crate::polkit::service::PolkitService;
use crate::power::service::{PowerService, PowerServiceConfig};
use crate::power_profiles::service::{PowerProfilesService, PowerProfilesServiceConfig};
use crate::session_lock::service::SessionLockService;
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
    pub media: MediaServiceConfig,
    pub menu: MenuServiceConfig,
    pub notifications: NotificationServiceConfig,
    pub auth: AuthServiceConfig,
    pub power: PowerServiceConfig,
}

/// Capacity of every command channel in the bridge.
///
/// Commands are user actions, so a small bound is plenty. When a service is
/// slow the router applies backpressure to the service channel, and the public
/// command channel never grows without bound.
const COMMAND_CAPACITY: usize = 64;

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

        let (commands, public_commands) = async_channel::bounded(COMMAND_CAPACITY);
        // Events stay unbounded because several services use `try_send` for
        // them and would silently drop state updates under load.
        let (service_events, events) = async_channel::unbounded();
        let (workspace_commands, workspace_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (monitor_commands, monitor_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (audio_commands, audio_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (brightness_commands, brightness_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (battery_commands, battery_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (stats_commands, stats_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (idle_inhibit_commands, idle_inhibit_receiver) =
            async_channel::bounded(COMMAND_CAPACITY);
        let (power_profiles_commands, power_profiles_receiver) =
            async_channel::bounded(COMMAND_CAPACITY);
        let (network_commands, network_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (tray_commands, tray_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (media_commands, media_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (menu_commands, menu_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (notification_commands, notification_receiver) =
            async_channel::bounded(COMMAND_CAPACITY);
        let (auth_commands, auth_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (polkit_commands, polkit_receiver) = async_channel::bounded(COMMAND_CAPACITY);
        let (power_commands, power_receiver) = async_channel::bounded(COMMAND_CAPACITY);

        let thread = thread::Builder::new()
            .name("nosh-core".to_string())
            .spawn(move || {
                runtime.block_on(async move {
                    let router = tokio::spawn(route_commands(
                        public_commands,
                        ServiceSenders {
                            by_service: [
                                workspace_commands,
                                monitor_commands,
                                audio_commands,
                                brightness_commands,
                                idle_inhibit_commands,
                                power_profiles_commands,
                                network_commands,
                                tray_commands,
                                media_commands,
                                menu_commands,
                                notification_commands,
                                auth_commands,
                                polkit_commands,
                                power_commands,
                            ],
                            _passive: vec![battery_commands, stats_commands],
                        },
                    ));
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
                    let media = MediaService::from_env_with(settings.media)
                        .run(media_receiver, service_events.clone());
                    let menu =
                        MenuService::new(settings.menu).run(menu_receiver, service_events.clone());
                    let notifications = NotificationService::from_env_with(settings.notifications)
                        .run(notification_receiver, service_events.clone());
                    let auth = AuthService::from_env_with(settings.auth)
                        .run(auth_receiver, service_events.clone());
                    let polkit =
                        PolkitService::from_env().run(polkit_receiver, service_events.clone());
                    let power = PowerService::from_env_with(settings.power)
                        .run(power_receiver, service_events.clone());
                    let session_lock = SessionLockService::from_env().run(service_events.clone());
                    // The services own a clone. Dropping the original closes the
                    // bus when every service returns; no forwarding task is needed.
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
                        media,
                        menu,
                        notifications,
                        auth,
                        polkit,
                        power,
                        session_lock
                    );
                    router.abort();
                    let _ = router.await;
                    if let Some(bind_sync) = bind_sync {
                        bind_sync.abort();
                        let _ = bind_sync.await;
                    }
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

    /// Queues a command without blocking.
    ///
    /// The public command channel is bounded, so a command is dropped (and
    /// logged) when the core is overwhelmed instead of growing memory without
    /// bound. A failure also means the core has stopped.
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
/// channel fans out to one service channel per service. The array is indexed by
/// [`Service`], so the order matches the enum. The battery and stats services
/// take no commands; their senders are held so their receivers stay open.
struct ServiceSenders {
    by_service: [Sender<Command>; Service::COUNT],
    _passive: Vec<Sender<Command>>,
}

async fn route_commands(commands: Receiver<Command>, senders: ServiceSenders) {
    while let Ok(command) = commands.recv().await {
        let index = command.service() as usize;
        if senders.by_service[index].send(command).await.is_err() {
            tracing::debug!("a service command channel is closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::monitors::LayoutPreset;

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
        let (media_tx, media_rx) = async_channel::unbounded();
        let (menu_tx, _menu_rx) = async_channel::unbounded();
        let (notification_tx, _notification_rx) = async_channel::unbounded();
        let (auth_tx, _auth_rx) = async_channel::unbounded();
        let (polkit_tx, _polkit_rx) = async_channel::unbounded();
        let (power_tx, _power_rx) = async_channel::unbounded();

        let task = tokio::spawn(route_commands(
            public_rx,
            ServiceSenders {
                by_service: [
                    workspace_tx,
                    monitor_tx,
                    audio_tx,
                    brightness_tx,
                    idle_inhibit_tx,
                    power_profiles_tx,
                    network_tx,
                    tray_tx,
                    media_tx,
                    menu_tx,
                    notification_tx,
                    auth_tx,
                    polkit_tx,
                    power_tx,
                ],
                _passive: vec![battery_tx, stats_tx],
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
        public_tx.send(Command::MediaPlayPause).await.unwrap();

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
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), media_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Command::MediaPlayPause
        );

        drop(public_tx);
        task.await.unwrap();
    }
}
