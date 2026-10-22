//! Monitor models, placement rules, layout presets, and the monitor service.
//!
//! Reads use a [`MonitorBackend`], and Hyprland is the only adapter in this
//! crate. The service resolves writes separately, so a compositor that only
//! supports reading still gets bars and notification placement.

pub mod backend;
pub mod model;
pub mod placement;
pub mod preset;
pub mod service;

pub use backend::{
    resolve_monitor, MonitorBackend, MonitorBackendEvent, MonitorBackendRef, MonitorCandidate,
    ResolvedMonitor,
};
pub use model::{MonitorCapabilities, MonitorId, MonitorLayout, MonitorModel, Transform};
pub use placement::{score, select_monitor, Corner, MonitorPolicy};
pub use preset::{extend, mirror, panning, preset_layout, resolve_target, single, LayoutPreset};
pub use service::{MonitorService, MonitorServiceConfig, StaticMonitorBackend};
