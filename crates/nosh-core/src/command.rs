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
}
