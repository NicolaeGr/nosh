use crate::audio::model::EndpointKind;
use crate::auth::model::Secret;
use crate::brightness::model::BrightnessDeviceId;
use crate::monitors::LayoutPreset;
use crate::monitors::MonitorLayout;
use crate::power::model::PowerAction;
use crate::power_profiles::model::PowerProfile;

/// User intent. Sending a command does not block the caller.
///
/// The monitor variants carry [`MonitorLayout`], which holds `f64` geometry, so
/// the enum is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AudioSetVolume(f64),
    AudioStepVolume(f64),
    AudioSetSourceVolume(f64),
    AudioSetEndpointVolume {
        kind: EndpointKind,
        id: u32,
        volume: f64,
    },
    AudioSetEndpointMuted {
        kind: EndpointKind,
        id: u32,
        muted: bool,
    },
    AudioSetMuted(bool),
    AudioToggleMute(EndpointKind),
    AudioSetDefault(u32),
    BrightnessSet(f64),
    BrightnessStep(f64),
    BrightnessSetTarget {
        device: BrightnessDeviceId,
        percent: f64,
    },
    BrightnessSnapshot,
    BrightnessDim {
        value: f64,
    },
    BrightnessRestore,
    BrightnessRescan,
    WorkspaceActivate(String),
    WorkspaceMoveToMonitor {
        workspace: String,
        monitor: String,
    },
    MonitorApply(MonitorLayout),
    MonitorCycle {
        target: String,
        mode: Option<LayoutPreset>,
    },
    MonitorMode {
        target: String,
        mode: LayoutPreset,
    },
    MonitorSetPrimary(String),
    MonitorSetEnabled {
        connector: String,
        enabled: bool,
    },
    IdleInhibitSet(bool),
    IdleInhibitToggle,
    PowerProfileSet(PowerProfile),
    PowerProfileCycle,
    WifiSetEnabled(bool),
    /// Ask NetworkManager for a scan. Sent when the network dropdown opens.
    WifiScan,
    /// Whether the model carries the access point list. False while the
    /// dropdown is closed, so the bus and the radio stay quiet.
    WifiSetDetails(bool),
    WifiActivate {
        ssid: String,
        password: Option<String>,
    },
    WifiDeactivate,
    WiredSetEnabled {
        interface: String,
        enabled: bool,
    },
    VpnActivate(String),
    VpnDeactivate(String),
    AirplaneModeSet(bool),
    NotificationDismiss(u32),
    /// The popup timeout ran out. The notification expired without the user
    /// acting on it.
    NotificationExpire(u32),
    NotificationInvoke {
        id: u32,
        action: String,
    },
    NotificationSetDnd(bool),
    /// Check a password. The reply is an [`crate::event::AuthEvent`].
    AuthAuthenticate {
        request_id: u64,
        username: String,
        password: Secret,
    },
    /// Answer a polkit prompt with the user's password.
    PolkitRespond {
        cookie: String,
        password: Secret,
    },
    /// Dismiss a polkit prompt without answering it.
    PolkitCancel {
        cookie: String,
    },
    /// Run a session action after the UI has authenticated the user.
    PowerAction(PowerAction),
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    /// Seek the current track to an absolute position in milliseconds.
    MediaSeek(u64),
    MediaSetVolume(f64),
    /// Whether the media view is on screen. The service polls the playback
    /// position only while it is. When hidden, the D-Bus watch still reports
    /// metadata changes and the bar keeps the title current.
    MediaSetVisible(bool),
    TrayActivate {
        item_id: String,
        x: i32,
        y: i32,
    },
    TraySecondaryActivate {
        item_id: String,
        x: i32,
        y: i32,
    },
    TrayScroll {
        item_id: String,
        delta: i32,
        horizontal: bool,
    },
    TrayMenuAboutToShow {
        item_id: String,
    },
    TrayMenuEvent {
        item_id: String,
        menu_id: i32,
        event: String,
    },
    MenuOpen(String),
    MenuQuery {
        id: String,
        query: String,
    },
    MenuActivate {
        id: String,
        index: usize,
        /// A named alternate action, or `None` for the default action.
        action: Option<String>,
    },
    MenuReload(String),
    MenuClose(String),
}

/// The idle inhibitor command behind a routed [`Command`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleInhibitCommand {
    Set(bool),
    Toggle,
}

/// The service that handles a command.
///
/// The bridge indexes its command senders by this value, so the order matches
/// the sender array there. Every command has exactly one owner.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Workspaces = 0,
    Monitors,
    Audio,
    Brightness,
    IdleInhibit,
    PowerProfiles,
    Network,
    Tray,
    Media,
    Menu,
    Notifications,
    Auth,
    Polkit,
    Power,
}

impl Service {
    pub const COUNT: usize = 14;
}

impl Command {
    /// The service that handles this command.
    ///
    /// This match is exhaustive, so a new command cannot be added without
    /// choosing its owner.
    pub fn service(&self) -> Service {
        match self {
            Self::WorkspaceActivate(_) | Self::WorkspaceMoveToMonitor { .. } => Service::Workspaces,
            Self::MonitorApply(_)
            | Self::MonitorCycle { .. }
            | Self::MonitorMode { .. }
            | Self::MonitorSetPrimary(_)
            | Self::MonitorSetEnabled { .. } => Service::Monitors,
            Self::AudioSetVolume(_)
            | Self::AudioStepVolume(_)
            | Self::AudioSetSourceVolume(_)
            | Self::AudioSetEndpointVolume { .. }
            | Self::AudioSetEndpointMuted { .. }
            | Self::AudioSetMuted(_)
            | Self::AudioToggleMute(_)
            | Self::AudioSetDefault(_) => Service::Audio,
            Self::BrightnessSet(_)
            | Self::BrightnessStep(_)
            | Self::BrightnessSetTarget { .. }
            | Self::BrightnessSnapshot
            | Self::BrightnessDim { .. }
            | Self::BrightnessRestore
            | Self::BrightnessRescan => Service::Brightness,
            Self::IdleInhibitSet(_) | Self::IdleInhibitToggle => Service::IdleInhibit,
            Self::PowerProfileSet(_) | Self::PowerProfileCycle => Service::PowerProfiles,
            Self::WifiSetEnabled(_)
            | Self::WifiScan
            | Self::WifiSetDetails(_)
            | Self::WifiActivate { .. }
            | Self::WifiDeactivate
            | Self::WiredSetEnabled { .. }
            | Self::VpnActivate(_)
            | Self::VpnDeactivate(_)
            | Self::AirplaneModeSet(_) => Service::Network,
            Self::TrayActivate { .. }
            | Self::TraySecondaryActivate { .. }
            | Self::TrayScroll { .. }
            | Self::TrayMenuAboutToShow { .. }
            | Self::TrayMenuEvent { .. } => Service::Tray,
            Self::MediaPlayPause
            | Self::MediaNext
            | Self::MediaPrevious
            | Self::MediaSeek(_)
            | Self::MediaSetVolume(_)
            | Self::MediaSetVisible(_) => Service::Media,
            Self::MenuOpen(_)
            | Self::MenuQuery { .. }
            | Self::MenuActivate { .. }
            | Self::MenuReload(_)
            | Self::MenuClose(_) => Service::Menu,
            Self::NotificationDismiss(_)
            | Self::NotificationExpire(_)
            | Self::NotificationInvoke { .. }
            | Self::NotificationSetDnd(_) => Service::Notifications,
            Self::AuthAuthenticate { .. } => Service::Auth,
            Self::PolkitRespond { .. } | Self::PolkitCancel { .. } => Service::Polkit,
            Self::PowerAction(_) => Service::Power,
        }
    }

    /// The idle inhibitor command, if this is one.
    pub fn idle_inhibit(&self) -> Option<IdleInhibitCommand> {
        match self {
            Self::IdleInhibitSet(value) => Some(IdleInhibitCommand::Set(*value)),
            Self::IdleInhibitToggle => Some(IdleInhibitCommand::Toggle),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_routes_to_one_service() {
        let auth = || Command::AuthAuthenticate {
            request_id: 1,
            username: "user".to_string(),
            password: Secret::new("x"),
        };
        let cases = vec![
            (
                Command::WorkspaceActivate("1".to_string()),
                Service::Workspaces,
            ),
            (
                Command::WorkspaceMoveToMonitor {
                    workspace: "1".to_string(),
                    monitor: "DP-1".to_string(),
                },
                Service::Workspaces,
            ),
            (
                Command::MonitorApply(MonitorLayout::default()),
                Service::Monitors,
            ),
            (
                Command::MonitorCycle {
                    target: "1".to_string(),
                    mode: None,
                },
                Service::Monitors,
            ),
            (
                Command::MonitorMode {
                    target: "DP-1".to_string(),
                    mode: LayoutPreset::Mirror,
                },
                Service::Monitors,
            ),
            (
                Command::MonitorSetPrimary("DP-1".to_string()),
                Service::Monitors,
            ),
            (
                Command::MonitorSetEnabled {
                    connector: "DP-1".to_string(),
                    enabled: false,
                },
                Service::Monitors,
            ),
            (Command::AudioSetVolume(0.5), Service::Audio),
            (Command::AudioStepVolume(-0.05), Service::Audio),
            (Command::AudioSetSourceVolume(0.5), Service::Audio),
            (
                Command::AudioSetEndpointVolume {
                    kind: EndpointKind::Sink,
                    id: 3,
                    volume: 0.5,
                },
                Service::Audio,
            ),
            (
                Command::AudioSetEndpointMuted {
                    kind: EndpointKind::Source,
                    id: 4,
                    muted: true,
                },
                Service::Audio,
            ),
            (Command::AudioSetMuted(true), Service::Audio),
            (Command::AudioToggleMute(EndpointKind::Sink), Service::Audio),
            (Command::AudioSetDefault(3), Service::Audio),
            (Command::BrightnessSet(50.0), Service::Brightness),
            (Command::BrightnessStep(5.0), Service::Brightness),
            (
                Command::BrightnessSetTarget {
                    device: BrightnessDeviceId::Backlight("intel_backlight".to_string()),
                    percent: 40.0,
                },
                Service::Brightness,
            ),
            (Command::BrightnessSnapshot, Service::Brightness),
            (Command::BrightnessDim { value: 10.0 }, Service::Brightness),
            (Command::BrightnessRestore, Service::Brightness),
            (Command::BrightnessRescan, Service::Brightness),
            (Command::IdleInhibitSet(true), Service::IdleInhibit),
            (Command::IdleInhibitToggle, Service::IdleInhibit),
            (
                Command::PowerProfileSet(PowerProfile::Performance),
                Service::PowerProfiles,
            ),
            (Command::PowerProfileCycle, Service::PowerProfiles),
            (Command::WifiSetEnabled(true), Service::Network),
            (Command::WifiScan, Service::Network),
            (Command::WifiSetDetails(false), Service::Network),
            (
                Command::WifiActivate {
                    ssid: "home".to_string(),
                    password: None,
                },
                Service::Network,
            ),
            (Command::WifiDeactivate, Service::Network),
            (
                Command::WiredSetEnabled {
                    interface: "eth0".to_string(),
                    enabled: false,
                },
                Service::Network,
            ),
            (Command::VpnActivate("uuid".to_string()), Service::Network),
            (Command::VpnDeactivate("uuid".to_string()), Service::Network),
            (Command::AirplaneModeSet(true), Service::Network),
            (
                Command::TrayActivate {
                    item_id: ":1.2/StatusNotifierItem".to_string(),
                    x: 0,
                    y: 0,
                },
                Service::Tray,
            ),
            (
                Command::TraySecondaryActivate {
                    item_id: "item".to_string(),
                    x: 1,
                    y: 2,
                },
                Service::Tray,
            ),
            (
                Command::TrayScroll {
                    item_id: "item".to_string(),
                    delta: 1,
                    horizontal: false,
                },
                Service::Tray,
            ),
            (
                Command::TrayMenuAboutToShow {
                    item_id: "item".to_string(),
                },
                Service::Tray,
            ),
            (
                Command::TrayMenuEvent {
                    item_id: "item".to_string(),
                    menu_id: 3,
                    event: "clicked".to_string(),
                },
                Service::Tray,
            ),
            (Command::MediaPlayPause, Service::Media),
            (Command::MediaNext, Service::Media),
            (Command::MediaPrevious, Service::Media),
            (Command::MediaSeek(1_000), Service::Media),
            (Command::MediaSetVolume(0.4), Service::Media),
            (Command::MediaSetVisible(true), Service::Media),
            (Command::MenuOpen("apps".to_string()), Service::Menu),
            (
                Command::MenuQuery {
                    id: "apps".to_string(),
                    query: "fire".to_string(),
                },
                Service::Menu,
            ),
            (
                Command::MenuActivate {
                    id: "apps".to_string(),
                    index: 0,
                    action: None,
                },
                Service::Menu,
            ),
            (Command::MenuReload("apps".to_string()), Service::Menu),
            (Command::MenuClose("apps".to_string()), Service::Menu),
            (Command::NotificationDismiss(3), Service::Notifications),
            (Command::NotificationExpire(3), Service::Notifications),
            (
                Command::NotificationInvoke {
                    id: 3,
                    action: "default".to_string(),
                },
                Service::Notifications,
            ),
            (Command::NotificationSetDnd(true), Service::Notifications),
            (auth(), Service::Auth),
            (
                Command::PolkitRespond {
                    cookie: "cookie".to_string(),
                    password: Secret::new("x"),
                },
                Service::Polkit,
            ),
            (
                Command::PolkitCancel {
                    cookie: "cookie".to_string(),
                },
                Service::Polkit,
            ),
            (Command::PowerAction(PowerAction::Shutdown), Service::Power),
        ];

        for (command, service) in cases {
            assert_eq!(command.service(), service, "{command:?}");
        }
    }

    #[test]
    fn idle_inhibit_commands_carry_their_value() {
        assert_eq!(
            Command::IdleInhibitSet(false).idle_inhibit(),
            Some(IdleInhibitCommand::Set(false))
        );
        assert_eq!(
            Command::IdleInhibitToggle.idle_inhibit(),
            Some(IdleInhibitCommand::Toggle)
        );
        assert_eq!(Command::AudioSetVolume(0.5).idle_inhibit(), None);
    }
}
