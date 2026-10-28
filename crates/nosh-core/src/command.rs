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

impl Command {
    /// Whether the audio service handles this command.
    pub fn is_audio(&self) -> bool {
        matches!(
            self,
            Self::AudioSetVolume(_)
                | Self::AudioStepVolume(_)
                | Self::AudioSetSourceVolume(_)
                | Self::AudioSetEndpointVolume { .. }
                | Self::AudioSetEndpointMuted { .. }
                | Self::AudioSetMuted(_)
                | Self::AudioToggleMute(_)
                | Self::AudioSetDefault(_)
        )
    }

    /// Whether the brightness service handles this command.
    pub fn is_brightness(&self) -> bool {
        matches!(
            self,
            Self::BrightnessSet(_)
                | Self::BrightnessStep(_)
                | Self::BrightnessSetTarget { .. }
                | Self::BrightnessSnapshot
                | Self::BrightnessDim { .. }
                | Self::BrightnessRestore
                | Self::BrightnessRescan
        )
    }

    /// Whether the monitor service handles this command.
    pub fn is_monitor(&self) -> bool {
        matches!(
            self,
            Self::MonitorApply(_)
                | Self::MonitorCycle { .. }
                | Self::MonitorMode { .. }
                | Self::MonitorSetPrimary(_)
                | Self::MonitorSetEnabled { .. }
        )
    }

    /// Whether the media service handles this command.
    pub fn is_media(&self) -> bool {
        matches!(
            self,
            Self::MediaPlayPause
                | Self::MediaNext
                | Self::MediaPrevious
                | Self::MediaSeek(_)
                | Self::MediaSetVolume(_)
        )
    }

    /// Whether the notification service handles this command.
    pub fn is_notifications(&self) -> bool {
        matches!(
            self,
            Self::NotificationDismiss(_)
                | Self::NotificationExpire(_)
                | Self::NotificationInvoke { .. }
                | Self::NotificationSetDnd(_)
        )
    }

    /// Whether the auth service handles this command.
    pub fn is_auth(&self) -> bool {
        matches!(self, Self::AuthAuthenticate { .. })
    }

    /// Whether the polkit agent handles this command.
    pub fn is_polkit(&self) -> bool {
        matches!(self, Self::PolkitRespond { .. } | Self::PolkitCancel { .. })
    }

    /// Whether the power service handles this command.
    pub fn is_power(&self) -> bool {
        matches!(self, Self::PowerAction(_))
    }

    /// Whether the tray service handles this command.
    pub fn is_tray(&self) -> bool {
        matches!(
            self,
            Self::TrayActivate { .. }
                | Self::TraySecondaryActivate { .. }
                | Self::TrayScroll { .. }
                | Self::TrayMenuAboutToShow { .. }
                | Self::TrayMenuEvent { .. }
        )
    }

    /// Whether the menu service handles this command.
    pub fn is_menu(&self) -> bool {
        matches!(
            self,
            Self::MenuOpen(_)
                | Self::MenuQuery { .. }
                | Self::MenuActivate { .. }
                | Self::MenuReload(_)
                | Self::MenuClose(_)
        )
    }

    /// Whether the power profiles service handles this command.
    pub fn is_power_profiles(&self) -> bool {
        matches!(self, Self::PowerProfileSet(_) | Self::PowerProfileCycle)
    }

    /// Whether the network service handles this command.
    pub fn is_network(&self) -> bool {
        matches!(
            self,
            Self::WifiSetEnabled(_)
                | Self::WifiScan
                | Self::WifiSetDetails(_)
                | Self::WifiActivate { .. }
                | Self::WifiDeactivate
                | Self::WiredSetEnabled { .. }
                | Self::VpnActivate(_)
                | Self::VpnDeactivate(_)
                | Self::AirplaneModeSet(_)
        )
    }

    /// Whether the idle inhibitor service handles this command.
    pub fn is_idle_inhibit(&self) -> bool {
        self.idle_inhibit().is_some()
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
    fn media_commands_are_classified_for_routing() {
        assert!(Command::MediaPlayPause.is_media());
        assert!(Command::MediaNext.is_media());
        assert!(Command::MediaPrevious.is_media());
        assert!(Command::MediaSeek(1_000).is_media());
        assert!(Command::MediaSetVolume(0.4).is_media());

        assert!(!Command::AudioSetVolume(0.5).is_media());
        assert!(!Command::NotificationDismiss(3).is_media());
    }

    #[test]
    fn notification_commands_are_classified_for_routing() {
        assert!(Command::NotificationDismiss(3).is_notifications());
        assert!(Command::NotificationExpire(3).is_notifications());
        assert!(Command::NotificationInvoke {
            id: 3,
            action: "default".to_string(),
        }
        .is_notifications());
        assert!(Command::NotificationSetDnd(true).is_notifications());

        assert!(!Command::IdleInhibitToggle.is_notifications());
        assert!(!Command::AudioSetVolume(0.5).is_notifications());
    }

    #[test]
    fn menu_commands_are_classified_for_routing() {
        assert!(Command::MenuOpen("apps".to_string()).is_menu());
        assert!(Command::MenuQuery {
            id: "apps".to_string(),
            query: "fire".to_string(),
        }
        .is_menu());
        assert!(Command::MenuActivate {
            id: "apps".to_string(),
            index: 0,
        }
        .is_menu());
        assert!(Command::MenuReload("apps".to_string()).is_menu());
        assert!(Command::MenuClose("apps".to_string()).is_menu());

        assert!(!Command::IdleInhibitToggle.is_menu());
        assert!(!Command::AudioSetVolume(0.5).is_menu());
    }

    #[test]
    fn monitor_commands_are_classified_for_routing() {
        assert!(Command::MonitorApply(MonitorLayout::default()).is_monitor());
        assert!(Command::MonitorCycle {
            target: "1".to_string(),
            mode: None,
        }
        .is_monitor());
        assert!(Command::MonitorMode {
            target: "DP-1".to_string(),
            mode: LayoutPreset::Mirror,
        }
        .is_monitor());
        assert!(Command::MonitorSetPrimary("DP-1".to_string()).is_monitor());
        assert!(Command::MonitorSetEnabled {
            connector: "DP-1".to_string(),
            enabled: false,
        }
        .is_monitor());

        assert!(!Command::WorkspaceActivate("1".to_string()).is_monitor());
        assert!(!Command::WorkspaceMoveToMonitor {
            workspace: "1".to_string(),
            monitor: "DP-1".to_string(),
        }
        .is_monitor());
    }

    #[test]
    fn brightness_commands_are_classified_for_routing() {
        assert!(Command::BrightnessSet(50.0).is_brightness());
        assert!(Command::BrightnessStep(5.0).is_brightness());
        assert!(Command::BrightnessSetTarget {
            device: BrightnessDeviceId::Backlight("intel_backlight".to_string()),
            percent: 40.0,
        }
        .is_brightness());
        assert!(Command::BrightnessSnapshot.is_brightness());
        assert!(Command::BrightnessDim { value: 10.0 }.is_brightness());
        assert!(Command::BrightnessRestore.is_brightness());
        assert!(Command::BrightnessRescan.is_brightness());

        assert!(!Command::AudioSetVolume(0.5).is_brightness());
        assert!(!Command::MonitorSetPrimary("DP-1".to_string()).is_brightness());
    }

    #[test]
    fn audio_commands_are_classified_for_routing() {
        assert!(Command::AudioSetVolume(0.5).is_audio());
        assert!(Command::AudioStepVolume(-0.05).is_audio());
        assert!(Command::AudioSetSourceVolume(0.5).is_audio());
        assert!(Command::AudioSetEndpointVolume {
            kind: EndpointKind::Sink,
            id: 3,
            volume: 0.5,
        }
        .is_audio());
        assert!(Command::AudioSetEndpointMuted {
            kind: EndpointKind::Source,
            id: 4,
            muted: true,
        }
        .is_audio());
        assert!(Command::AudioSetMuted(true).is_audio());
        assert!(Command::AudioToggleMute(EndpointKind::Sink).is_audio());
        assert!(Command::AudioSetDefault(3).is_audio());

        assert!(!Command::WorkspaceActivate("1".to_string()).is_audio());
        assert!(!Command::MonitorSetPrimary("DP-1".to_string()).is_audio());
    }

    #[test]
    fn auth_and_power_commands_are_classified_for_routing() {
        assert!(Command::AuthAuthenticate {
            request_id: 1,
            username: "user".to_string(),
            password: Secret::new("x"),
        }
        .is_auth());
        assert!(Command::PowerAction(PowerAction::Shutdown).is_power());

        assert!(Command::PolkitRespond {
            cookie: "cookie".to_string(),
            password: Secret::new("x"),
        }
        .is_polkit());
        assert!(Command::PolkitCancel {
            cookie: "cookie".to_string(),
        }
        .is_polkit());

        assert!(!Command::PolkitCancel {
            cookie: "cookie".to_string(),
        }
        .is_auth());
        assert!(!Command::PowerAction(PowerAction::Shutdown).is_polkit());

        assert!(!Command::PowerAction(PowerAction::Shutdown).is_auth());
        assert!(!Command::AuthAuthenticate {
            request_id: 1,
            username: "user".to_string(),
            password: Secret::new("x"),
        }
        .is_power());
    }

    #[test]
    fn tray_commands_are_classified_for_routing() {
        assert!(Command::TrayActivate {
            item_id: ":1.2/StatusNotifierItem".to_string(),
            x: 0,
            y: 0,
        }
        .is_tray());
        assert!(Command::TraySecondaryActivate {
            item_id: "item".to_string(),
            x: 1,
            y: 2,
        }
        .is_tray());
        assert!(Command::TrayScroll {
            item_id: "item".to_string(),
            delta: 1,
            horizontal: false,
        }
        .is_tray());
        assert!(Command::TrayMenuAboutToShow {
            item_id: "item".to_string(),
        }
        .is_tray());
        assert!(Command::TrayMenuEvent {
            item_id: "item".to_string(),
            menu_id: 3,
            event: "clicked".to_string(),
        }
        .is_tray());

        assert!(!Command::IdleInhibitToggle.is_tray());
        assert!(!Command::AudioSetVolume(0.5).is_tray());
    }

    #[test]
    fn power_profile_commands_are_classified_for_routing() {
        assert!(Command::PowerProfileSet(PowerProfile::Performance).is_power_profiles());
        assert!(Command::PowerProfileCycle.is_power_profiles());

        assert!(!Command::IdleInhibitToggle.is_power_profiles());
        assert!(!Command::AudioSetVolume(0.5).is_power_profiles());
    }

    #[test]
    fn network_commands_are_classified_for_routing() {
        assert!(Command::WifiSetEnabled(true).is_network());
        assert!(Command::WifiScan.is_network());
        assert!(Command::WifiSetDetails(false).is_network());
        assert!(Command::WifiActivate {
            ssid: "home".to_string(),
            password: None,
        }
        .is_network());
        assert!(Command::WifiDeactivate.is_network());
        assert!(Command::WiredSetEnabled {
            interface: "eth0".to_string(),
            enabled: false,
        }
        .is_network());
        assert!(Command::VpnActivate("uuid".to_string()).is_network());
        assert!(Command::VpnDeactivate("uuid".to_string()).is_network());
        assert!(Command::AirplaneModeSet(true).is_network());

        assert!(!Command::AudioSetVolume(0.5).is_network());
        assert!(!Command::AirplaneModeSet(true).is_power_profiles());
    }

    #[test]
    fn idle_inhibit_commands_are_classified_for_routing() {
        assert!(Command::IdleInhibitSet(true).is_idle_inhibit());
        assert!(Command::IdleInhibitToggle.is_idle_inhibit());
        assert_eq!(
            Command::IdleInhibitSet(false).idle_inhibit(),
            Some(IdleInhibitCommand::Set(false))
        );
        assert_eq!(
            Command::IdleInhibitToggle.idle_inhibit(),
            Some(IdleInhibitCommand::Toggle)
        );

        assert!(!Command::AudioSetVolume(0.5).is_idle_inhibit());
        assert!(!Command::BrightnessSet(50.0).is_idle_inhibit());
    }
}
