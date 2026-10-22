//! System state for the nosh shell.
//!
//! This crate has no GTK or display server dependency. It talks to the
//! compositor, resolves backends, and publishes typed events over a channel.
//! The UI crate renders snapshots and sends commands.

pub mod audio;
pub mod backends;
pub mod backoff;
pub mod bridge;
pub mod brightness;
pub mod command;
pub mod error;
pub mod event;
pub mod hyprland;
pub mod model;
pub mod monitors;
pub mod origin;
pub mod paths;
pub mod resolver;
pub mod workspaces;

#[cfg(feature = "audio-pulse")]
pub use audio::PulseBackend;
pub use audio::{
    bar_step_volume, volume_icon, AudioBackend, AudioBackendEvent, AudioBackendRef, AudioCandidate,
    AudioCapabilities, AudioEndpoint, AudioModel, AudioService, AudioServiceConfig, EndpointKind,
    WpctlBackend, BAR_MAX_VOLUME, MAX_SOURCE_VOLUME, MAX_VOLUME, VOLUME_STEP,
};
pub use backends::{BackendEvent, BackendRef, WorkspaceBackend};
pub use bridge::{Core, CoreSettings};
pub use brightness::{
    clamp_percent, percent_from_raw, raw_from_percent, BrightnessBackend, BrightnessCandidate,
    BrightnessDevice, BrightnessDeviceId, BrightnessDeviceInfo, BrightnessEvent, BrightnessKind,
    BrightnessReading, BrightnessService, BrightnessServiceConfig, BrightnessSnapshotFile,
    BrightnessctlBackend, CommandBackend, CommandDeviceConfig, DdcBackend, DdcDisplay,
    DdcutilBackend, SysfsLogindBackend, BRIGHTNESS_STEP, DIM_PERCENT, GAMMA,
};
pub use command::Command;
pub use error::{
    BackendError, BrightnessError, CoreError, EventParseError, MonitorError, PathError,
};
pub use event::{AudioEvent, Event, MonitorEvent, Service, Snapshot, WorkspaceEvent};
pub use model::{WorkspaceCapabilities, WorkspaceModel, WorkspaceMonitor, WorkspaceSnapshot};
pub use monitors::{
    extend, mirror, panning, preset_layout, resolve_target, score, select_monitor, single, Corner,
    LayoutPreset, MonitorBackend, MonitorBackendEvent, MonitorCapabilities, MonitorId,
    MonitorLayout, MonitorModel, MonitorPolicy, MonitorService, MonitorServiceConfig, Transform,
};
pub use origin::ChangeOrigin;
pub use paths::{HyprlandPaths, NoshPaths};
pub use resolver::{Candidate, Resolved};
