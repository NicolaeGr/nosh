//! The session actions and what logind reports for them.

/// One session action from the power menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerAction {
    Shutdown,
    Reboot,
    Logout,
    Suspend,
    Hibernate,
}

impl PowerAction {
    /// Every action in menu order.
    pub const ALL: [Self; 5] = [
        Self::Logout,
        Self::Suspend,
        Self::Hibernate,
        Self::Reboot,
        Self::Shutdown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Shutdown => "Power off",
            Self::Reboot => "Restart",
            Self::Logout => "Log out",
            Self::Suspend => "Suspend",
            Self::Hibernate => "Hibernate",
        }
    }

    /// A short line under the label.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Shutdown => "Shut down",
            Self::Reboot => "Reboot",
            Self::Logout => "End the session",
            Self::Suspend => "Sleep",
            Self::Hibernate => "Save to disk",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Shutdown => "system-shutdown-symbolic",
            Self::Reboot => "system-reboot-symbolic",
            Self::Logout => "system-log-out-symbolic",
            Self::Suspend => "system-suspend-symbolic",
            Self::Hibernate => "system-hibernate-symbolic",
        }
    }

    /// Whether the action ends the session or loses work without a restart.
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Shutdown | Self::Reboot)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shutdown => "shutdown",
            Self::Reboot => "reboot",
            Self::Logout => "logout",
            Self::Suspend => "suspend",
            Self::Hibernate => "hibernate",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str().eq_ignore_ascii_case(word))
    }
}

/// Which actions logind accepts on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PowerModel {
    pub supported: Vec<PowerAction>,
    /// Actions logind answers with `challenge`, so the shell must ask for a
    /// password before sending them.
    pub requires_auth: Vec<PowerAction>,
}

impl PowerModel {
    pub fn supports(&self, action: PowerAction) -> bool {
        self.supported.contains(&action)
    }

    pub fn requires_auth(&self, action: PowerAction) -> bool {
        self.requires_auth.contains(&action)
    }

    pub fn any(&self) -> bool {
        !self.supported.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_word_and_round_trips() {
        for action in PowerAction::ALL {
            assert_eq!(PowerAction::parse(action.as_str()), Some(action));
        }
    }

    #[test]
    fn a_model_reports_only_the_actions_it_holds() {
        let model = PowerModel {
            supported: vec![PowerAction::Shutdown, PowerAction::Logout],
            requires_auth: vec![PowerAction::Shutdown],
        };
        assert!(model.supports(PowerAction::Shutdown));
        assert!(!model.supports(PowerAction::Hibernate));
        assert!(model.requires_auth(PowerAction::Shutdown));
        assert!(!model.requires_auth(PowerAction::Logout));
        assert!(model.any());
    }

    #[test]
    fn destructive_actions_are_marked() {
        assert!(PowerAction::Shutdown.is_destructive());
        assert!(PowerAction::Reboot.is_destructive());
        assert!(!PowerAction::Suspend.is_destructive());
    }
}
