//! The power profiles as the UI renders them.

/// One profile exposed by `power-profiles-daemon`.
///
/// Unknown daemon values are kept instead of dropped, so a newer backend does
/// not break the control.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PowerProfile {
    PowerSaver,
    #[default]
    Balanced,
    Performance,
    Unknown(String),
}

impl PowerProfile {
    pub fn parse(value: &str) -> Self {
        match value {
            "power-saver" => Self::PowerSaver,
            "balanced" => Self::Balanced,
            "performance" => Self::Performance,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::PowerSaver => "power-saver",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::Unknown(value) => value,
        }
    }

    /// The label shown in the toggle and the profile list.
    pub fn label(&self) -> &str {
        match self {
            Self::PowerSaver => "Power Saver",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::Unknown(value) => value,
        }
    }

    pub fn icon_name(&self) -> String {
        format!("power-profile-{}-symbolic", self.as_str())
    }

    /// Balanced is the neutral profile, so the toggle shows it as off.
    pub fn is_balanced(&self) -> bool {
        matches!(self, Self::Balanced)
    }
}

impl std::fmt::Display for PowerProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The complete power profile state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerProfilesModel {
    pub active: PowerProfile,
    pub profiles: Vec<PowerProfile>,
    /// The non-empty `PerformanceDegraded` reason, when the daemon reports one.
    pub degraded: Option<String>,
}

impl Default for PowerProfilesModel {
    fn default() -> Self {
        Self {
            active: PowerProfile::default(),
            profiles: default_profiles(),
            degraded: None,
        }
    }
}

/// The profiles the daemon exposes when it reports none, and the canonical
/// cycle order.
pub fn default_profiles() -> Vec<PowerProfile> {
    vec![
        PowerProfile::PowerSaver,
        PowerProfile::Balanced,
        PowerProfile::Performance,
    ]
}

/// The next profile in the daemon's list. An active profile the daemon no
/// longer lists falls back to the first available one.
pub fn next_profile(active: &PowerProfile, profiles: &[PowerProfile]) -> Option<PowerProfile> {
    if profiles.is_empty() {
        return None;
    }

    let index = profiles
        .iter()
        .position(|profile| profile == active)
        .map(|index| (index + 1) % profiles.len())
        .unwrap_or(0);
    Some(profiles[index].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_names_map_to_profiles() {
        assert_eq!(PowerProfile::parse("power-saver"), PowerProfile::PowerSaver);
        assert_eq!(PowerProfile::parse("balanced"), PowerProfile::Balanced);
        assert_eq!(
            PowerProfile::parse("performance"),
            PowerProfile::Performance
        );
        assert_eq!(
            PowerProfile::parse("turbo"),
            PowerProfile::Unknown("turbo".to_string())
        );
    }

    #[test]
    fn names_round_trip_through_the_wire_format() {
        for profile in default_profiles() {
            assert_eq!(PowerProfile::parse(profile.as_str()), profile);
        }
    }

    #[test]
    fn icons_follow_the_active_profile() {
        assert_eq!(
            PowerProfile::PowerSaver.icon_name(),
            "power-profile-power-saver-symbolic"
        );
        assert_eq!(
            PowerProfile::Balanced.icon_name(),
            "power-profile-balanced-symbolic"
        );
        assert_eq!(
            PowerProfile::Performance.icon_name(),
            "power-profile-performance-symbolic"
        );
    }

    #[test]
    fn balanced_is_the_neutral_profile() {
        assert!(PowerProfile::Balanced.is_balanced());
        assert!(!PowerProfile::PowerSaver.is_balanced());
        assert!(!PowerProfile::Performance.is_balanced());
    }

    #[test]
    fn cycling_follows_the_daemon_order_and_wraps() {
        let profiles = default_profiles();
        assert_eq!(
            next_profile(&PowerProfile::PowerSaver, &profiles),
            Some(PowerProfile::Balanced)
        );
        assert_eq!(
            next_profile(&PowerProfile::Balanced, &profiles),
            Some(PowerProfile::Performance)
        );
        assert_eq!(
            next_profile(&PowerProfile::Performance, &profiles),
            Some(PowerProfile::PowerSaver)
        );
    }

    #[test]
    fn cycling_skips_missing_profiles() {
        let profiles = vec![PowerProfile::PowerSaver, PowerProfile::Performance];
        assert_eq!(
            next_profile(&PowerProfile::PowerSaver, &profiles),
            Some(PowerProfile::Performance)
        );
        assert_eq!(
            next_profile(&PowerProfile::Performance, &profiles),
            Some(PowerProfile::PowerSaver)
        );
    }

    #[test]
    fn an_unknown_active_profile_cycles_to_the_first_available_one() {
        let profiles = default_profiles();
        assert_eq!(
            next_profile(&PowerProfile::Unknown("turbo".to_string()), &profiles),
            Some(PowerProfile::PowerSaver)
        );
        assert_eq!(next_profile(&PowerProfile::Balanced, &[]), None);
    }
}
