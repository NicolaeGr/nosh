//! Brightness across the internal panel, DDC/CI monitors, and user commands.
//!
//! The service is an actor. It owns a queue, resolves a backend per device, and
//! coalesces slider movement before any I/O happens. Internal panels use sysfs
//! reads with logind writes, falling back to `brightnessctl`. External displays
//! use DDC/CI through `ddcutil`.

pub mod backend;
pub mod brightnessctl;
pub mod command;
pub mod ddc;
pub mod model;
mod process;
pub mod service;
pub mod snapshot;
pub mod sysfs;

pub use backend::{
    discover_first, resolve_device, BoundBackend, BrightnessBackend, BrightnessBackendRef,
    BrightnessCandidate, BrightnessCapabilities,
};
pub use brightnessctl::{BrightnessctlBackend, BrightnessctlConfig};
pub use command::{CommandBackend, CommandDeviceConfig};
pub use ddc::{
    parse_detect, parse_getvcp, DdcBackend, DdcBackendBridge, DdcDisplay, DdcValue, DdcutilBackend,
    DEFAULT_DDC_TIMEOUT, VCP_BRIGHTNESS,
};
pub use model::{
    clamp_percent, linear_percent_from_raw, linear_raw_from_percent, percent_from_raw,
    raw_from_percent, round_percent, BrightnessDevice, BrightnessDeviceId, BrightnessDeviceInfo,
    BrightnessEvent, BrightnessKind, BrightnessReading, BrightnessSnapshotEntry,
    BrightnessSnapshotFile, BRIGHTNESS_STEP, DIM_PERCENT, GAMMA, PERCENT_EPSILON,
};
pub use service::{BrightnessService, BrightnessServiceConfig};
pub use snapshot::{load as load_snapshot, remove as remove_snapshot, save as save_snapshot};
pub use sysfs::{backlight_score, SysfsLogindBackend, SysfsLogindConfig, SYSFS_ROOT};
