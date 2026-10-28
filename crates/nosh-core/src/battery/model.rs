//! Battery state as the UI renders it.
//!
//! UPower reports percentage as `0..100`. The core normalizes it to `0..=1` and
//! computes the icon name, so the UI only copies fields.

use std::time::Duration;

/// UPower device type for a battery.
pub const TYPE_LINE_POWER: u32 = 1;
/// UPower device type for a battery.
pub const TYPE_BATTERY: u32 = 2;

/// A missing or unrecognized battery icon.
pub const ICON_MISSING: &str = "battery-missing-symbolic";
/// The icon for a full battery that is still charging.
pub const ICON_FULL_CHARGED: &str = "battery-level-100-charged-symbolic";

/// The UPower `State` property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum BatteryState {
    #[default]
    Unknown,
    Charging,
    Discharging,
    Empty,
    FullyCharged,
    PendingCharge,
    PendingDischarge,
}

impl BatteryState {
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::Charging,
            2 => Self::Discharging,
            3 => Self::Empty,
            4 => Self::FullyCharged,
            5 => Self::PendingCharge,
            6 => Self::PendingDischarge,
            _ => Self::Unknown,
        }
    }

    /// Charging and fully charged share the charging icon suffix.
    pub const fn is_charging(self) -> bool {
        matches!(self, Self::Charging | Self::FullyCharged)
    }

    /// The state as the UI shows it. Unknown has no label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Empty => "Empty",
            Self::FullyCharged => "Fully charged",
            Self::PendingCharge => "Pending charge",
            Self::PendingDischarge => "Pending discharge",
            Self::Unknown => "",
        }
    }
}

/// The battery state the UI renders.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryModel {
    pub present: bool,
    pub is_battery: bool,
    /// `0.0..=1.0`.
    pub percentage: f64,
    pub state: BatteryState,
    pub charging: bool,
    pub icon_name: String,
    pub time_to_empty: Option<Duration>,
    pub time_to_full: Option<Duration>,
}

impl Default for BatteryModel {
    fn default() -> Self {
        Self {
            present: false,
            is_battery: false,
            percentage: 0.0,
            state: BatteryState::Unknown,
            charging: false,
            icon_name: ICON_MISSING.to_string(),
            time_to_empty: None,
            time_to_full: None,
        }
    }
}

impl BatteryModel {
    /// The time left in the current state, when UPower reports one.
    pub fn remaining(&self) -> Option<Duration> {
        match self.state {
            BatteryState::Charging | BatteryState::PendingCharge => self.time_to_full,
            BatteryState::Discharging | BatteryState::PendingDischarge => self.time_to_empty,
            _ => None,
        }
    }

    /// `Charging · 1h 18m`, or just the state when UPower has no estimate.
    pub fn status_text(&self) -> String {
        match (self.state.label(), self.remaining()) {
            ("", _) => String::new(),
            (label, None) => label.to_string(),
            (label, Some(duration)) => format!("{label} · {}", format_duration(duration)),
        }
    }

    /// The tooltip text. The percentage is floored for parity with the Vala
    /// widget and the state and estimate follow on a second line.
    pub fn tooltip(&self) -> String {
        tooltip(self)
    }
}

/// The raw UPower `org.freedesktop.UPower.Device` properties this service uses.
#[derive(Debug, Clone, PartialEq)]
pub struct BatteryProperties {
    pub kind: u32,
    /// `0.0..=100.0` as UPower reports it.
    pub percentage: f64,
    pub state: u32,
    pub present: bool,
    pub power_supply: bool,
    /// Seconds, or zero when unknown.
    pub time_to_empty: i64,
    /// Seconds, or zero when unknown.
    pub time_to_full: i64,
}

impl Default for BatteryProperties {
    fn default() -> Self {
        Self {
            kind: 0,
            percentage: 0.0,
            state: 0,
            present: false,
            power_supply: false,
            time_to_empty: 0,
            time_to_full: 0,
        }
    }
}

impl BatteryProperties {
    pub fn model(&self) -> BatteryModel {
        let state = BatteryState::from_raw(self.state);
        let mut model = BatteryModel {
            present: self.present,
            is_battery: self.kind == TYPE_BATTERY,
            percentage: normalize_percentage(self.percentage),
            state,
            charging: state.is_charging(),
            icon_name: String::new(),
            time_to_empty: seconds(self.time_to_empty),
            time_to_full: seconds(self.time_to_full),
        };
        model.icon_name = icon_name(&model);
        model
    }
}

/// Clamps UPower's `0..100` percentage and divides it by 100.
pub fn normalize_percentage(percentage: f64) -> f64 {
    if percentage.is_finite() {
        percentage.clamp(0.0, 100.0) / 100.0
    } else {
        0.0
    }
}

fn seconds(value: i64) -> Option<Duration> {
    (value > 0).then(|| Duration::from_secs(value as u64))
}

/// The icon for a battery model.
///
/// A full, charging battery uses a different icon from a full, idle one. The
/// level rounds to the nearest ten percent.
pub fn icon_name(model: &BatteryModel) -> String {
    if !model.is_battery {
        ICON_MISSING.to_string()
    } else if model.percentage >= 0.95 && model.charging {
        ICON_FULL_CHARGED.to_string()
    } else {
        let level = (model.percentage * 10.0).round() as i32 * 10;
        let suffix = if model.charging { "-charging" } else { "" };
        format!("battery-level-{level}{suffix}-symbolic")
    }
}

/// `Battery: {floor(percentage * 100)}%`. The floor matches the Vala tooltip.
///
/// A state line follows when UPower reports one:
/// `Battery: 32%\nCharging · 1h 18m until full`.
pub fn tooltip(model: &BatteryModel) -> String {
    let percent = (model.percentage * 100.0).floor() as i64;
    let mut text = format!("Battery: {percent}%");
    let status = model.status_text();
    if status.is_empty() {
        return text;
    }

    text.push('\n');
    text.push_str(&status);
    if model.remaining().is_some() {
        match model.state {
            BatteryState::Charging | BatteryState::PendingCharge => text.push_str(" until full"),
            BatteryState::Discharging | BatteryState::PendingDischarge => {
                text.push_str(" remaining")
            }
            _ => {}
        }
    }
    text
}

/// `4h 12m` or `42m`.
pub fn format_duration(duration: Duration) -> String {
    let minutes = duration.as_secs() / 60;
    let hours = minutes / 60;
    let minutes = minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laptop(percentage: f64, state: BatteryState) -> BatteryModel {
        BatteryProperties {
            kind: TYPE_BATTERY,
            percentage: percentage * 100.0,
            state: match state {
                BatteryState::Charging => 1,
                BatteryState::Discharging => 2,
                BatteryState::Empty => 3,
                BatteryState::FullyCharged => 4,
                BatteryState::PendingCharge => 5,
                BatteryState::PendingDischarge => 6,
                BatteryState::Unknown => 0,
            },
            present: true,
            power_supply: true,
            time_to_empty: 0,
            time_to_full: 0,
        }
        .model()
    }

    #[test]
    fn percentage_is_clamped_and_normalized() {
        assert_eq!(normalize_percentage(54.6), 0.546);
        assert_eq!(normalize_percentage(0.0), 0.0);
        assert_eq!(normalize_percentage(-10.0), 0.0);
        assert_eq!(normalize_percentage(180.0), 1.0);
        assert_eq!(normalize_percentage(f64::NAN), 0.0);
        assert_eq!(normalize_percentage(f64::INFINITY), 0.0);
    }

    #[test]
    fn percentages_below_the_full_branch_round_to_ten_percent_steps() {
        assert_eq!(
            laptop(0.949, BatteryState::Discharging).icon_name,
            "battery-level-90-symbolic"
        );
        assert_eq!(
            laptop(0.55, BatteryState::Discharging).icon_name,
            "battery-level-60-symbolic"
        );
        assert_eq!(
            laptop(0.0, BatteryState::Discharging).icon_name,
            "battery-level-0-symbolic"
        );
    }

    #[test]
    fn a_nearly_full_charging_battery_uses_the_charged_icon() {
        assert_eq!(
            laptop(0.95, BatteryState::Charging).icon_name,
            ICON_FULL_CHARGED
        );
        assert_eq!(
            laptop(1.0, BatteryState::FullyCharged).icon_name,
            ICON_FULL_CHARGED
        );
    }

    #[test]
    fn a_full_battery_that_is_not_charging_uses_the_flat_icon() {
        assert_eq!(
            laptop(0.95, BatteryState::Discharging).icon_name,
            "battery-level-100-symbolic"
        );
    }

    #[test]
    fn a_charging_battery_gets_the_charging_suffix() {
        assert_eq!(
            laptop(0.42, BatteryState::Charging).icon_name,
            "battery-level-40-charging-symbolic"
        );
        // Pending states are not charging yet.
        assert_eq!(
            laptop(0.42, BatteryState::PendingCharge).icon_name,
            "battery-level-40-symbolic"
        );
    }

    #[test]
    fn a_line_power_device_is_not_a_battery() {
        let model = BatteryProperties {
            kind: TYPE_LINE_POWER,
            percentage: 100.0,
            state: 4,
            present: true,
            power_supply: true,
            ..BatteryProperties::default()
        }
        .model();

        assert!(!model.is_battery);
        assert_eq!(model.icon_name, ICON_MISSING);
    }

    #[test]
    fn states_map_to_charging_and_the_rest() {
        assert!(BatteryState::from_raw(1).is_charging());
        assert!(BatteryState::from_raw(4).is_charging());
        assert!(!BatteryState::from_raw(2).is_charging());
        assert_eq!(BatteryState::from_raw(99), BatteryState::Unknown);
    }

    #[test]
    fn the_tooltip_floors_the_percentage() {
        let model = laptop(0.999, BatteryState::Discharging);
        assert_eq!(tooltip(&model), "Battery: 99%\nDischarging");
        assert_eq!(
            tooltip(&laptop(0.54, BatteryState::Charging)),
            "Battery: 54%\nCharging"
        );
        assert_eq!(
            tooltip(&laptop(1.0, BatteryState::FullyCharged)),
            "Battery: 100%\nFully charged"
        );
    }

    fn with_times(
        percentage: f64,
        state: BatteryState,
        time_to_empty: i64,
        time_to_full: i64,
    ) -> BatteryModel {
        BatteryProperties {
            kind: TYPE_BATTERY,
            percentage: percentage * 100.0,
            state: match state {
                BatteryState::Charging => 1,
                BatteryState::Discharging => 2,
                BatteryState::Empty => 3,
                BatteryState::FullyCharged => 4,
                BatteryState::PendingCharge => 5,
                BatteryState::PendingDischarge => 6,
                BatteryState::Unknown => 0,
            },
            present: true,
            power_supply: true,
            time_to_empty,
            time_to_full,
        }
        .model()
    }

    #[test]
    fn status_text_uses_the_state_appropriate_estimate() {
        let charging = with_times(0.32, BatteryState::Charging, 0, 78 * 60);
        assert_eq!(charging.remaining(), Some(Duration::from_secs(78 * 60)));
        assert_eq!(charging.status_text(), "Charging · 1h 18m");
        assert_eq!(
            charging.tooltip(),
            "Battery: 32%\nCharging · 1h 18m until full"
        );

        let discharging = with_times(0.84, BatteryState::Discharging, 3 * 3600 + 42 * 60, 0);
        assert_eq!(discharging.status_text(), "Discharging · 3h 42m");
        assert_eq!(
            discharging.tooltip(),
            "Battery: 84%\nDischarging · 3h 42m remaining"
        );
    }

    #[test]
    fn fully_charged_has_no_estimate() {
        let model = with_times(1.0, BatteryState::FullyCharged, 0, 3600);
        assert_eq!(model.remaining(), None);
        assert_eq!(model.status_text(), "Fully charged");
        assert_eq!(model.tooltip(), "Battery: 100%\nFully charged");
    }

    #[test]
    fn an_unknown_state_adds_no_tooltip_line() {
        let model = with_times(0.5, BatteryState::Unknown, 3600, 3600);
        assert_eq!(model.status_text(), "");
        assert_eq!(model.tooltip(), "Battery: 50%");
    }

    #[test]
    fn durations_use_hours_and_minutes() {
        assert_eq!(
            format_duration(Duration::from_secs(4 * 3600 + 12 * 60)),
            "4h 12m"
        );
        assert_eq!(format_duration(Duration::from_secs(42 * 60)), "42m");
        assert_eq!(format_duration(Duration::from_secs(0)), "0m");
    }

    #[test]
    fn zero_times_become_none() {
        let model = BatteryProperties {
            kind: TYPE_BATTERY,
            percentage: 50.0,
            time_to_empty: 0,
            time_to_full: 0,
            ..BatteryProperties::default()
        }
        .model();
        assert_eq!(model.time_to_empty, None);
        assert_eq!(model.time_to_full, None);

        let model = BatteryProperties {
            time_to_empty: 3600,
            time_to_full: 7200,
            ..BatteryProperties::default()
        }
        .model();
        assert_eq!(model.time_to_empty, Some(Duration::from_secs(3600)));
        assert_eq!(model.time_to_full, Some(Duration::from_secs(7200)));
    }
}
