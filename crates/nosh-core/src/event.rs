use crate::audio::model::{AudioEndpoint, AudioModel, EndpointKind};
use crate::auth::model::AuthResult;
use crate::battery::model::BatteryModel;
use crate::brightness::model::{BrightnessDevice, BrightnessDeviceId};
use crate::idle_inhibit::model::IdleInhibitState;
use crate::media::model::MediaModel;
use crate::menu::MenuEvent;
use crate::monitors::MonitorLayout;
use crate::network::model::NetworkModel;
use crate::notifications::model::{ClosedReason, Notification, NotificationModel};
use crate::origin::ChangeOrigin;
use crate::power::model::{PowerAction, PowerModel};
use crate::power_profiles::model::PowerProfilesModel;
use crate::stats::model::SystemStats;
use crate::tray::model::{TrayItemModel, TrayMenu};
use crate::workspaces::model::WorkspaceSnapshot;

/// Every service publishes state through this enum.
///
/// A snapshot carries the complete state of every service, so it is much
/// larger than a single-service event by design.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Event {
    /// Complete state for every service that has started.
    Snapshot(Snapshot),
    Workspaces(WorkspaceEvent),
    Monitors(MonitorEvent),
    Audio(AudioEvent),
    Brightness(BrightnessEvent),
    Battery(BatteryEvent),
    SystemStats(SystemStatsEvent),
    IdleInhibit(IdleInhibitEvent),
    PowerProfiles(PowerProfilesEvent),
    Network(NetworkEvent),
    Tray(TrayEvent),
    Media(MediaEvent),
    Notifications(NotificationEvent),
    Auth(AuthEvent),
    Power(PowerEvent),
    Menu(MenuEvent),
    ServiceAvailability(Service, bool),
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub workspaces: Option<WorkspaceSnapshot>,
    pub workspace_backend: Option<&'static str>,
    pub monitors: Option<MonitorLayout>,
    pub monitor_backend: Option<&'static str>,
    pub audio: Option<AudioModel>,
    pub audio_backend: Option<&'static str>,
    pub brightness: Option<Vec<crate::brightness::model::BrightnessDevice>>,
    pub battery: Option<BatteryModel>,
    pub system_stats: Option<SystemStats>,
    pub idle_inhibit: Option<IdleInhibitState>,
    pub idle_inhibit_backend: Option<&'static str>,
    pub power_profiles: Option<PowerProfilesModel>,
    pub network: Option<NetworkModel>,
    pub network_backend: Option<&'static str>,
    pub tray: Option<Vec<TrayItemModel>>,
    pub tray_backend: Option<&'static str>,
    pub media: Option<MediaModel>,
    pub media_backend: Option<&'static str>,
    pub notifications: Option<NotificationModel>,
    pub notification_backend: Option<&'static str>,
    pub power: Option<PowerModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Service {
    Workspaces,
    Monitors,
    Audio,
    Brightness,
    Battery,
    SystemStats,
    IdleInhibit,
    PowerProfiles,
    Network,
    Tray,
    Media,
    Notifications,
    Auth,
    Power,
}

#[derive(Debug, Clone)]
pub enum WorkspaceEvent {
    /// Fresh state after a resync or reconnect.
    Snapshot {
        state: WorkspaceSnapshot,
        backend: &'static str,
    },
    FocusedChanged {
        workspace: Option<String>,
    },
    UrgentChanged {
        workspace: String,
        urgent: bool,
    },
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}

#[derive(Debug, Clone)]
pub enum MonitorEvent {
    /// Fresh state after a resync or reconnect.
    Snapshot {
        layout: MonitorLayout,
        backend: &'static str,
    },
    /// The compositor reported a monitor change. The layout is complete.
    LayoutChanged(MonitorLayout),
    /// A write was rejected or could not be delivered.
    ApplyFailed(String),
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}

#[derive(Debug, Clone)]
pub enum AudioEvent {
    /// Fresh state after startup or a backend reconnect.
    Snapshot(AudioModel),
    VolumeChanged {
        kind: EndpointKind,
        id: u32,
        volume: f64,
        old: f64,
        origin: ChangeOrigin,
    },
    MuteChanged {
        kind: EndpointKind,
        id: u32,
        muted: bool,
        origin: ChangeOrigin,
    },
    DefaultChanged {
        kind: EndpointKind,
        endpoint: AudioEndpoint,
    },
    /// A write was rejected or could not be delivered.
    WriteFailed(String),
    /// A read failed after a change notification.
    ReadFailed(String),
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}

#[derive(Debug, Clone)]
pub enum BrightnessEvent {
    /// Complete device list after startup, a rescan, or a device change.
    Snapshot(Vec<BrightnessDevice>),
    DeviceChanged {
        device: BrightnessDeviceId,
        percent: f64,
        old: f64,
        origin: ChangeOrigin,
    },
    /// A value is queued but not written yet.
    Pending {
        device: BrightnessDeviceId,
        target: f64,
    },
    Restored,
    Unavailable,
}

#[derive(Debug, Clone)]
pub enum BatteryEvent {
    /// Fresh state after startup or a reconnect.
    Snapshot(BatteryModel),
    /// One or more display device properties changed. The model is complete.
    Changed(BatteryModel),
    /// UPower is missing or reports no battery.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatsEvent {
    /// A fresh CPU and memory sample.
    Changed(SystemStats),
}

#[derive(Debug, Clone)]
pub enum TrayEvent {
    /// The complete item list after startup or a reconnect.
    Snapshot(Vec<TrayItemModel>),
    /// A new item was read. Its menu is already fetched or was unavailable.
    ItemAdded {
        item: TrayItemModel,
        menu: Option<TrayMenu>,
    },
    /// An existing item changed one of its properties.
    ItemUpdated {
        item: TrayItemModel,
    },
    ItemRemoved {
        item_id: String,
    },
    /// The DBusMenu tree changed. `None` means the item has no usable menu.
    MenuUpdated {
        item_id: String,
        menu: Option<TrayMenu>,
    },
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}

/// The power profile state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerProfilesEvent {
    /// Fresh state after startup or a daemon reconnect.
    Snapshot(PowerProfilesModel),
    /// The daemon accepted a new profile or reported a property change.
    Changed(PowerProfilesModel),
    /// The daemon is missing or the connection is down.
    Unavailable,
}

/// The network state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    /// Fresh state after startup or a NetworkManager reconnect.
    Snapshot(NetworkModel),
    /// The state changed. The model is complete for the requested detail level.
    Changed(NetworkModel),
    /// A scan started. The next changed event carries the new access points.
    ScanStarted,
    /// A scan finished or timed out.
    ScanFinished,
    /// A user action failed, for example a wrong password.
    OperationFailed(String),
    /// NetworkManager is missing or the connection is down.
    Unavailable,
}

/// The now-playing player.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaEvent {
    /// Fresh state after startup or a backend reconnect.
    Snapshot(MediaModel),
    /// The track, playback state, position, or controls changed.
    Changed(MediaModel),
    /// A transport command failed.
    WriteFailed(String),
    /// The session bus is missing or left.
    Unavailable,
}

/// The notification daemon and store.
#[derive(Debug, Clone, PartialEq)]
pub enum NotificationEvent {
    /// Fresh state after startup, a reconnect, or a backend change.
    Snapshot(NotificationModel),
    /// A notification was stored. `suppress_popup` is true under DND.
    Received {
        notification: Notification,
        replaced: bool,
        suppress_popup: bool,
    },
    /// A notification left the store.
    Closed {
        id: u32,
        reason: ClosedReason,
    },
    /// An action was invoked from this shell.
    Invoked {
        id: u32,
        action: String,
    },
    DndChanged {
        enabled: bool,
    },
    BackendChanged {
        backend: &'static str,
    },
    Unavailable,
}

/// The result of one password check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthEvent {
    Finished(AuthResult),
}

/// The power menu actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    /// Which actions logind accepts on this machine.
    Snapshot(PowerModel),
    /// An authenticated action was sent to logind.
    ActionStarted(PowerAction),
    /// logind accepted the action. The session may end before the UI sees it.
    ActionCompleted(PowerAction),
    /// logind rejected the action or the call failed.
    ActionFailed {
        action: PowerAction,
        message: String,
    },
    /// logind is missing or the system bus is down.
    Unavailable,
}

/// The idle inhibitor state.
///
/// This feature prevents the compositor from going idle. It is not an idle
/// notifier and has no idle or resumed events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleInhibitEvent {
    /// Fresh state after startup, a reconnect, or the protocol appearing.
    Snapshot { available: bool, active: bool },
    /// The compositor accepted a new inhibitor state.
    Changed { active: bool },
    /// The protocol is missing or the connection is down. The state is inactive.
    Unavailable,
}
