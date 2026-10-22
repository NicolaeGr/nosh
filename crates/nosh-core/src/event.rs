use crate::audio::model::{AudioEndpoint, AudioModel, EndpointKind};
use crate::battery::model::BatteryModel;
use crate::brightness::model::{BrightnessDevice, BrightnessDeviceId};
use crate::monitors::MonitorLayout;
use crate::origin::ChangeOrigin;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Service {
    Workspaces,
    Monitors,
    Audio,
    Brightness,
    Battery,
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
