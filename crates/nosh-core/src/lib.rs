//! System state for the nosh shell.
//!
//! The crate has no GTK or display server dependency. It reads compositor
//! state, resolves backends, and publishes typed events over a channel. The UI
//! renders snapshots and sends commands.

pub mod audio;
pub mod auth;
pub mod backoff;
pub mod battery;
pub mod bridge;
pub mod brightness;
pub mod command;
pub mod error;
pub mod event;
pub mod hyprland;
pub mod idle_inhibit;
pub mod media;
pub mod menu;
pub mod monitors;
pub mod network;
pub mod notifications;
pub mod origin;
pub mod paths;
pub mod polkit;
pub mod power;
pub mod power_profiles;
pub mod session_lock;
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
pub use auth::{
    AuthBackend, AuthBackendRef, AuthRequest, AuthResult, AuthService, AuthServiceConfig,
    PamBackend, Secret,
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
    BackendError, BrightnessError, CoreError, EventParseError, MonitorError, NotificationError,
    PathError, StatsError,
};
pub use event::{
    AudioEvent, AuthEvent, BatteryEvent, BrightnessEvent, Event, IdleInhibitEvent, MediaEvent,
    MonitorEvent, NetworkEvent, NotificationEvent, PowerEvent, PowerProfilesEvent, Service,
    SessionLockEvent, Snapshot, SystemStatsEvent, TrayEvent, WorkspaceEvent,
};
pub use idle_inhibit::{
    IdleInhibitBackend, IdleInhibitService, IdleInhibitServiceConfig, IdleInhibitState,
    InhibitBackendEvent, InhibitCommand, InhibitSession, WaylandBackend,
};
pub use media::{
    MediaBackend, MediaBackendEvent, MediaBackendRef, MediaModel, MediaService, MediaServiceConfig,
    MprisBackend, PlaybackStatus,
};
pub use menu::{
    default_menus, expand_argv, load_menu_config, load_menus, menu_source, parse_menus,
    spawn_action, spawn_detached, ActionDef, After, ItemLayout, ItemState, LaunchMode, MenuConfig,
    MenuConfigError, MenuDef, MenuEvent, MenuIcon, MenuItem, MenuPreview, MenuService,
    MenuServiceConfig, MenuSource, MenuValue, SearchMode, SourceDef, SourceEnv, SourceError,
    SourceKind, Template, ViewDef, ViewKind, WindowDef,
};
pub use monitors::{
    extend, mirror, panning, preset_layout, resolve_target, score, select_monitor, single, Corner,
    LayoutPreset, MonitorBackend, MonitorBackendEvent, MonitorCapabilities, MonitorId,
    MonitorLayout, MonitorModel, MonitorPolicy, MonitorService, MonitorServiceConfig, Transform,
};
pub use network::{
    group_access_points, read_rfkill_devices, rfkill_available, security_from_flags, signal_icon,
    sort_networks, ssid_to_utf8, wifi_icon, AccessPointModel, AirplaneModeModel, Connectivity,
    DeviceState, NetworkBackend, NetworkBackendEvent, NetworkBackendRef, NetworkManagerBackend,
    NetworkModel, NetworkService, NetworkServiceConfig, RfkillDevice, RfkillKind, SecurityType,
    VpnKind, VpnModel, VpnProfileModel, WifiMode, WifiModel, WiredDeviceModel, RFKILL_ROOT,
};
pub use notifications::{
    parse_actions, restore as restore_notifications, ClosedReason, DaemonRole, Notification,
    NotificationAction, NotificationBackend, NotificationBackendRef, NotificationCandidate,
    NotificationModel, NotificationService, NotificationServiceConfig, NotificationSettings,
    NotificationState, NotificationStore, OwnDaemonBackend, Urgency, DEFAULT_CAPABILITIES,
};
pub use origin::ChangeOrigin;
pub use paths::{HyprlandPaths, NoshPaths};
pub use polkit::{
    AgentRequest, HelperMessage, HelperSession, Identity, PolkitAgentBackend, PolkitBackend,
    PolkitBackendRef, PolkitEvent, PolkitPrompt, PolkitService, HELPER_SOCKET,
    POLKIT_AGENT_INTERFACE, POLKIT_AGENT_PATH, POLKIT_AUTHORITY_INTERFACE, POLKIT_AUTHORITY_PATH,
    POLKIT_SERVICE,
};
pub use power::{
    LogindBackend, PowerAction, PowerBackend, PowerBackendRef, PowerModel, PowerService,
    PowerServiceConfig, LOGIND_MANAGER, LOGIND_PATH, LOGIND_SERVICE, LOGIND_SESSION_INTERFACE,
    LOGIND_SESSION_PATH,
};
pub use power_profiles::{
    default_profiles, next_profile, DbusPowerProfilesBackend, PowerProfile, PowerProfilesBackend,
    PowerProfilesBackendEvent, PowerProfilesBackendRef, PowerProfilesModel, PowerProfilesService,
    PowerProfilesServiceConfig, POWER_PROFILES_INTERFACE, POWER_PROFILES_PATH,
    POWER_PROFILES_SERVICE,
};
pub use session_lock::{
    LockMarker, LogindSessionLockBackend, SessionLockBackend, SessionLockBackendRef,
    SessionLockService, LOCK_MARKER_FILE,
};
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
