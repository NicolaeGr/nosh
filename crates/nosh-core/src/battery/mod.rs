//! Battery state from UPower.
//!
//! The core reads the display device and normalizes the percentage to
//! `0.0..=1.0`. The icon name and the tooltip are computed here so the UI only
//! copies two fields.

pub mod backend;
pub mod model;
pub mod service;
pub mod upower;

pub use crate::event::BatteryEvent;
pub use backend::{
    resolve_battery, BatteryBackend, BatteryBackendEvent, BatteryBackendRef, BatteryCandidate,
    ResolvedBattery,
};
pub use model::{
    icon_name, normalize_percentage, tooltip, BatteryModel, BatteryProperties, BatteryState,
    ICON_FULL_CHARGED, ICON_MISSING, TYPE_BATTERY, TYPE_LINE_POWER,
};
pub use service::{BatteryService, BatteryServiceConfig};
pub use upower::{
    UpowerBackend, DEVICE_INTERFACE, DISPLAY_DEVICE_PATH, PROPERTIES_INTERFACE, UPOWER_INTERFACE,
    UPOWER_PATH, UPOWER_SERVICE,
};
