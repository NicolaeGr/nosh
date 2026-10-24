use crate::audio::model::{AudioEndpoint, AudioModel, EndpointKind};
use crate::battery::model::BatteryModel;
use crate::brightness::model::{BrightnessDevice, BrightnessDeviceId};
use crate::idle_inhibit::model::IdleInhibitState;
use crate::monitors::MonitorLayout;
use crate::origin::ChangeOrigin;
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
    Tray(TrayEvent),
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
    pub tray: Option<Vec<TrayItemModel>>,
    pub tray_backend: Option<&'static str>,
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
    Tray,
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
