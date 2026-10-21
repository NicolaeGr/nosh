//! Audio sinks, sources, volume, mute, and the backend try tree.
//!
//! The Pulse backend is the default. `wpctl` is the command fallback, and the
//! null backend keeps the shell running with no audio server.

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
    bar_step_volume, round_volume, volume_icon, AudioEndpoint, AudioModel, ChangeOrigin,
    EndpointKind, BAR_MAX_VOLUME, MAX_SOURCE_VOLUME, MAX_VOLUME, VOLUME_EPSILON, VOLUME_STEP,
};
#[cfg(feature = "audio-pulse")]
pub use pulse::PulseBackend;
pub use service::{AudioService, AudioServiceConfig, NullAudioBackend};
pub use wpctl::{WpctlBackend, WpctlConfig};
