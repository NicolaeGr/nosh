//! Audio sinks, sources, volume, and mute.
//!
//! Pulse is the default backend. `wpctl` is the command fallback, and the
//! static backend keeps the shell running when no audio server is available.

pub mod backend;
pub mod model;
#[cfg(feature = "audio-pulse")]
pub mod pulse;
pub mod service;
pub mod wpctl;

pub use backend::{
    resolve_audio, AudioBackend, AudioBackendEvent, AudioBackendRef, AudioCandidate,
    AudioCapabilities, ResolvedAudio,
};
pub use model::{
    bar_step_volume, round_volume, volume_icon, AudioEndpoint, AudioModel, EndpointKind,
    BAR_MAX_VOLUME, MAX_SOURCE_VOLUME, MAX_VOLUME, VOLUME_EPSILON, VOLUME_STEP,
};
#[cfg(feature = "audio-pulse")]
pub use pulse::PulseBackend;
pub use service::{AudioService, AudioServiceConfig, StaticAudioBackend};
pub use wpctl::{WpctlBackend, WpctlConfig};

pub use crate::origin::ChangeOrigin;
