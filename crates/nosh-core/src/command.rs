use crate::brightness::model::BrightnessDeviceId;
use crate::monitors::LayoutPreset;
use crate::monitors::MonitorLayout;

/// User intent. Commands are cheap to send and never block the caller.
///
/// Variants are added with the feature that needs them. The monitor variants
/// are routed to the monitor service by [`crate::Core`].
///
/// `MonitorLayout` contains `f64` geometry, so the enum is `PartialEq` but not
/// `Eq`.
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
}

impl Command {
    /// Whether the audio service owns this command.
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

    /// Whether the brightness service owns this command.
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

    /// Whether the monitor service owns this command.
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
}
