//! Brightness devices, the gamma curve, and snapshot entries.
//!
//! Percent values are `0.0..=100.0`. Internal backlights use the same square
//! curve as `brightnessctl --exponent=2.0`. DDC/CI uses the display's own scale.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::BrightnessError;
use crate::origin::ChangeOrigin;

/// The exponent used by `brightnessctl --exponent=2.0` and by direct sysfs
/// writes. Keep it in one place so the slider and the CLI agree.
pub const GAMMA: f64 = 2.0;
/// One keyboard step.
pub const BRIGHTNESS_STEP: f64 = 5.0;
/// The default dim value.
pub const DIM_PERCENT: f64 = 10.0;
/// A difference smaller than this is not reported as a change.
pub const PERCENT_EPSILON: f64 = 0.05;

/// Where a brightness device lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrightnessKind {
    Internal,
    ExternalDdc,
    ExternalCommand,
}

impl BrightnessKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::ExternalDdc => "external",
            Self::ExternalCommand => "command",
        }
    }

    pub const fn is_internal(self) -> bool {
        matches!(self, Self::Internal)
    }
}

/// A stable device identifier.
///
/// The string form is persisted in the brightness snapshot:
/// `backlight:<name>`, `ddc:<bus>:<display>`, and `command:<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BrightnessDeviceId {
    /// A sysfs backlight name, for example `intel_backlight`.
    Backlight(String),
    /// An I2C bus and the connector or monitor name on it.
    Ddc { bus: u8, display: String },
    /// A user-configured command target.
    Named(String),
}

impl BrightnessDeviceId {
    pub fn stable_id(&self) -> String {
        self.to_string()
    }

    pub fn kind(&self) -> BrightnessKind {
        match self {
            Self::Backlight(_) => BrightnessKind::Internal,
            Self::Ddc { .. } => BrightnessKind::ExternalDdc,
            Self::Named(_) => BrightnessKind::ExternalCommand,
        }
    }

    pub fn ddc_bus(&self) -> Option<u8> {
        match self {
            Self::Ddc { bus, .. } => Some(*bus),
            _ => None,
        }
    }
}

impl fmt::Display for BrightnessDeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backlight(name) => write!(formatter, "backlight:{name}"),
            Self::Ddc { bus, display } => write!(formatter, "ddc:{bus}:{display}"),
            Self::Named(name) => write!(formatter, "command:{name}"),
        }
    }
}

impl FromStr for BrightnessDeviceId {
    type Err = BrightnessError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (prefix, rest) = value
            .split_once(':')
            .ok_or_else(|| BrightnessError::Parse(format!("invalid device id {value:?}")))?;

        match prefix {
            "backlight" if !rest.is_empty() => Ok(Self::Backlight(rest.to_string())),
            "command" if !rest.is_empty() => Ok(Self::Named(rest.to_string())),
            "ddc" => {
                let (bus, display) = rest.split_once(':').ok_or_else(|| {
                    BrightnessError::Parse(format!("invalid DDC device id {value:?}"))
                })?;
                let bus = bus
                    .parse::<u8>()
                    .map_err(|_| BrightnessError::Parse(format!("invalid DDC bus in {value:?}")))?;
                if display.is_empty() {
                    return Err(BrightnessError::Parse(format!(
                        "invalid DDC display in {value:?}"
                    )));
                }
                Ok(Self::Ddc {
                    bus,
                    display: display.to_string(),
                })
            }
            _ => Err(BrightnessError::Parse(format!(
                "unknown brightness device id {value:?}"
            ))),
        }
    }
}

/// One controllable display or backlight.
#[derive(Debug, Clone, PartialEq)]
pub struct BrightnessDevice {
    pub id: BrightnessDeviceId,
    pub kind: BrightnessKind,
    pub name: String,
    /// Compositor output name when it can be matched, for example `DP-1`.
    pub connector: Option<String>,
    /// `0.0..=100.0`.
    pub percent: f64,
    /// A value the UI may show before the actor confirms the write.
    pub pending_percent: Option<f64>,
    pub available: bool,
    pub raw: Option<u32>,
    pub max_raw: Option<u32>,
    pub ddc_bus: Option<u8>,
    /// The backend that currently owns the device.
    pub backend: Option<&'static str>,
}

/// The result of reading a device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrightnessReading {
    pub percent: f64,
    pub raw: Option<u32>,
    pub max_raw: Option<u32>,
}

impl BrightnessReading {
    pub fn percent(percent: f64) -> Self {
        Self {
            percent: clamp_percent(percent),
            raw: None,
            max_raw: None,
        }
    }
}

/// A device discovered by a backend before it is bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrightnessDeviceInfo {
    pub id: BrightnessDeviceId,
    pub kind: BrightnessKind,
    pub name: String,
    pub connector: Option<String>,
    pub ddc_bus: Option<u8>,
}

impl BrightnessDeviceInfo {
    pub fn internal(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: BrightnessDeviceId::Backlight(name.clone()),
            kind: BrightnessKind::Internal,
            name,
            connector: None,
            ddc_bus: None,
        }
    }

    pub fn command(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: BrightnessDeviceId::Named(name.clone()),
            kind: BrightnessKind::ExternalCommand,
            name,
            connector: None,
            ddc_bus: None,
        }
    }
}

/// Rounds to two decimal places so repeated steps do not drift.
pub fn round_percent(percent: f64) -> f64 {
    (percent * 100.0).round() / 100.0
}

pub fn clamp_percent(percent: f64) -> f64 {
    round_percent(percent.clamp(0.0, 100.0))
}

/// Converts a raw sysfs value to a percentage through the square curve.
pub fn percent_from_raw(raw: u32, max: u32) -> f64 {
    if max == 0 {
        return 0.0;
    }
    let ratio = (raw as f64 / max as f64).clamp(0.0, 1.0);
    round_percent(ratio.powf(1.0 / GAMMA) * 100.0)
}

/// Converts a percentage to a raw sysfs value through the square curve.
pub fn raw_from_percent(percent: f64, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let fraction = clamp_percent(percent) / 100.0;
    (max as f64 * fraction.powf(GAMMA))
        .round()
        .clamp(0.0, max as f64) as u32
}

/// DDC/CI displays use their own linear scale. Do not apply the gamma curve.
pub fn linear_percent_from_raw(raw: u32, max: u32) -> f64 {
    if max == 0 {
        return 0.0;
    }
    round_percent((raw as f64 / max as f64).clamp(0.0, 1.0) * 100.0)
}

pub fn linear_raw_from_percent(percent: f64, max: u32) -> u32 {
    if max == 0 {
        return 0;
    }
    let fraction = clamp_percent(percent) / 100.0;
    (max as f64 * fraction).round().clamp(0.0, max as f64) as u32
}

/// One entry in `~/.cache/nosh/brightness/snapshot.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrightnessSnapshotEntry {
    pub id: String,
    pub kind: String,
    pub percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u32>,
}

impl BrightnessSnapshotEntry {
    pub fn from_device(device: &BrightnessDevice) -> Self {
        let external = device.kind == BrightnessKind::ExternalDdc;
        Self {
            id: device.id.stable_id(),
            kind: device.kind.label().to_string(),
            percent: device.percent,
            raw: if external { None } else { device.raw },
            bus: device.ddc_bus,
            connector: device.connector.clone(),
            value: if external { device.raw } else { None },
        }
    }

    /// Stable ID first, then connector, then the DDC bus.
    pub fn matches(&self, device: &BrightnessDevice) -> bool {
        self.id == device.id.stable_id()
    }

    pub fn matches_connector(&self, device: &BrightnessDevice) -> bool {
        match (&self.connector, &device.connector) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => false,
        }
    }

    pub fn matches_bus(&self, device: &BrightnessDevice) -> bool {
        match (self.bus, device.ddc_bus) {
            (Some(expected), Some(actual)) => expected == actual,
            _ => false,
        }
    }
}

/// The persisted snapshot file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrightnessSnapshotFile {
    pub created: u64,
    pub devices: Vec<BrightnessSnapshotEntry>,
}

impl BrightnessSnapshotFile {
    pub fn from_devices(devices: &[BrightnessDevice], created: u64) -> Self {
        Self {
            created,
            devices: devices
                .iter()
                .map(BrightnessSnapshotEntry::from_device)
                .collect(),
        }
    }
}

/// The brightness events published by the service.
#[derive(Debug, Clone)]
pub enum BrightnessEvent {
    /// Complete device list after startup, a rescan, or a device change.
    Snapshot(Vec<BrightnessDevice>),
    DeviceChanged {
        device: BrightnessDeviceId,
        percent: f64,
        old: f64,
        source: ChangeOrigin,
    },
    /// A value is queued but not written yet.
    Pending {
        device: BrightnessDeviceId,
        target: f64,
    },
    Restored,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gamma_curve_matches_the_documented_points() {
        let max = 1000;
        assert_eq!(raw_from_percent(0.0, max), 0);
        assert_eq!(raw_from_percent(10.0, max), 10);
        assert_eq!(raw_from_percent(50.0, max), 250);
        assert_eq!(raw_from_percent(100.0, max), 1000);

        assert_eq!(percent_from_raw(0, max), 0.0);
        assert_eq!(percent_from_raw(10, max), 10.0);
        assert_eq!(percent_from_raw(250, max), 50.0);
        assert_eq!(percent_from_raw(1000, max), 100.0);
    }

    #[test]
    fn extremes_clamp() {
        let max = 1000;
        assert_eq!(raw_from_percent(-10.0, max), 0);
        assert_eq!(raw_from_percent(150.0, max), max);
        assert_eq!(percent_from_raw(2000, max), 100.0);
        assert_eq!(percent_from_raw(10, 0), 0.0);
        assert_eq!(raw_from_percent(10.0, 0), 0);
    }

    #[test]
    fn ddc_values_stay_linear() {
        assert_eq!(linear_raw_from_percent(50.0, 100), 50);
        assert_eq!(linear_percent_from_raw(50, 100), 50.0);
        assert_ne!(
            raw_from_percent(50.0, 100),
            linear_raw_from_percent(50.0, 100)
        );
    }

    #[test]
    fn device_ids_round_trip() {
        let ids = [
            BrightnessDeviceId::Backlight("intel_backlight".to_string()),
            BrightnessDeviceId::Ddc {
                bus: 7,
                display: "DP-1".to_string(),
            },
            BrightnessDeviceId::Named("Desk lamp".to_string()),
        ];

        for id in ids {
            let parsed: BrightnessDeviceId = id.to_string().parse().unwrap();
            assert_eq!(parsed, id);
        }
    }

    #[test]
    fn invalid_device_ids_are_rejected() {
        assert!("display:1".parse::<BrightnessDeviceId>().is_err());
        assert!("ddc:not-a-bus:DP-1".parse::<BrightnessDeviceId>().is_err());
        assert!("ddc:7".parse::<BrightnessDeviceId>().is_err());
        assert!("backlight:".parse::<BrightnessDeviceId>().is_err());
    }

    fn ddc_device() -> BrightnessDevice {
        BrightnessDevice {
            id: BrightnessDeviceId::Ddc {
                bus: 7,
                display: "DP-1".to_string(),
            },
            kind: BrightnessKind::ExternalDdc,
            name: "DP-1".to_string(),
            connector: Some("DP-1".to_string()),
            percent: 80.0,
            pending_percent: None,
            available: true,
            raw: Some(160),
            max_raw: Some(200),
            ddc_bus: Some(7),
            backend: Some("ddcutil"),
        }
    }

    #[test]
    fn snapshot_entries_use_the_documented_field_names() {
        let entry = BrightnessSnapshotEntry::from_device(&ddc_device());
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"id\":\"ddc:7:DP-1\""));
        assert!(json.contains("\"bus\":7"));
        assert!(json.contains("\"value\":160"));
        assert!(!json.contains("\"raw\""));
    }

    #[test]
    fn snapshot_matching_falls_back_to_connector_and_bus() {
        let device = ddc_device();
        let mut entry = BrightnessSnapshotEntry::from_device(&device);
        assert!(entry.matches(&device));
        assert!(entry.matches_connector(&device));
        assert!(entry.matches_bus(&device));

        entry.id = "ddc:7:renamed".to_string();
        assert!(!entry.matches(&device));
        assert!(entry.matches_connector(&device));
        assert!(entry.matches_bus(&device));
    }
}
