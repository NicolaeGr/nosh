//! System state for the nosh shell.
//!
//! The crate has no GTK or display server dependency. It reads compositor
//! state, resolves backends, and publishes typed events over a channel. The UI
//! renders snapshots and sends commands.

pub mod audio;
pub mod backoff;
pub mod battery;
pub mod bridge;
pub mod brightness;
pub mod command;
pub mod error;
pub mod event;
pub mod hyprland;
pub mod idle_inhibit;
pub mod monitors;
pub mod origin;
pub mod paths;
pub mod stats;
pub mod tray;
pub mod workspaces;

#[cfg(feature = "audio-pulse")]
pub use audio::PulseBackend;
pub use audio::{
    bar_step_volume, volume_icon, AudioBackend, AudioBackendEvent, AudioBackendRef, AudioCandidate,
    AudioCapabilities, AudioEndpoint, AudioModel, AudioService, AudioServiceConfig, EndpointKind,
    StaticAudioBackend, WpctlBackend, BAR_MAX_VOLUME, MAX_SOURCE_VOLUME, MAX_VOLUME, VOLUME_STEP,
};
pub use battery::{
    icon_name, normalize_percentage, tooltip, BatteryBackend, BatteryBackendEvent,
    BatteryBackendRef, BatteryCandidate, BatteryModel, BatteryProperties, BatteryService,
    BatteryServiceConfig, BatteryState, ResolvedBattery, UpowerBackend,
};
pub use bridge::{Core, CoreSettings};
pub use brightness::{
    clamp_percent, percent_from_raw, raw_from_percent, BrightnessBackend, BrightnessCandidate,
    BrightnessDevice, BrightnessDeviceId, BrightnessDeviceInfo, BrightnessKind, BrightnessReading,
    BrightnessService, BrightnessServiceConfig, BrightnessSnapshotFile, BrightnessctlBackend,
    CommandBackend, CommandDeviceConfig, DdcBackend, DdcDisplay, DdcutilBackend,
    SysfsLogindBackend, BRIGHTNESS_STEP, DIM_PERCENT, GAMMA,
};
pub use command::{Command, IdleInhibitCommand};
pub use error::{
    BackendError, BrightnessError, CoreError, EventParseError, MonitorError, PathError, StatsError,
};
pub use event::{
    AudioEvent, BatteryEvent, BrightnessEvent, Event, IdleInhibitEvent, MonitorEvent, Service,
    Snapshot, SystemStatsEvent, TrayEvent, WorkspaceEvent,
};
pub use idle_inhibit::{
    IdleInhibitBackend, IdleInhibitService, IdleInhibitServiceConfig, IdleInhibitState,
    InhibitBackendEvent, InhibitCommand, InhibitSession, WaylandBackend,
};
pub use monitors::{
    extend, mirror, panning, preset_layout, resolve_target, score, select_monitor, single, Corner,
    LayoutPreset, MonitorBackend, MonitorBackendEvent, MonitorCapabilities, MonitorId,
    MonitorLayout, MonitorModel, MonitorPolicy, MonitorService, MonitorServiceConfig, Transform,
};
pub use origin::ChangeOrigin;
pub use paths::{HyprlandPaths, NoshPaths};
pub use stats::{
    cpu_percent, parse_cpu_stat, parse_meminfo, CpuTimes, MemoryInfo, ProcStatsSource, StatsSource,
    StatsSourceRef, SystemStats, SystemStatsService, SystemStatsServiceConfig, DEFAULT_INTERVAL,
    MEMINFO_PATH, STAT_PATH,
};
pub use tray::{
    decode_argb32, default_candidates, find_icon_file, largest_pixmap, pick_window, DbusMenuClient,
    HyprlandFocus, IconCache, IconPixmap, NoTrayBackend, OwnWatcherBackend, ProxyWatcherBackend,
    Registration, TrayCandidate, TrayCategory, TrayIcon, TrayItemModel, TrayMenu, TrayMenuItem,
    TrayMenuItemKind, TrayService, TrayServiceConfig, TrayStatus, TrayToggleState, TrayToggleType,
    WatcherBackend, WatcherBackendRef, WatcherChange, WatcherSession, WindowFocus, WindowFocusRef,
    WindowHint, MAX_MENU_DEPTH, MAX_MENU_ITEMS, MAX_PIXMAP_SIZE, MENU_TIMEOUT, WANTED_PROPERTIES,
};
pub use workspaces::{
    normalize_address, resolve_workspace, sort_workspaces, ResolvedWorkspace,
    StaticWorkspaceBackend, WorkspaceBackend, WorkspaceBackendEvent, WorkspaceBackendRef,
    WorkspaceCandidate, WorkspaceCapabilities, WorkspaceModel, WorkspaceMonitor, WorkspaceService,
    WorkspaceServiceConfig, WorkspaceSnapshot, WorkspaceState,
};
