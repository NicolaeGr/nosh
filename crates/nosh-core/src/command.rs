use crate::brightness::model::BrightnessDeviceId;
use crate::monitors::LayoutPreset;
use crate::monitors::MonitorLayout;

/// User intent. Sending a command does not block the caller.
///
/// The monitor variants carry [`MonitorLayout`], which holds `f64` geometry, so
/// the enum is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AudioSetVolume(f64),
    AudioStepVolume(f64),
    AudioSetSourceVolume(f64),
    AudioSetMuted(bool),
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
                | Self::AudioSetMuted(_)
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
        assert!(Command::AudioSetMuted(true).is_audio());
        assert!(Command::AudioSetDefault(3).is_audio());

        assert!(!Command::WorkspaceActivate("1".to_string()).is_audio());
        assert!(!Command::MonitorSetPrimary("DP-1".to_string()).is_audio());
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
