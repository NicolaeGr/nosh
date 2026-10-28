//! The now-playing player: MPRIS reading, watching, and transport control.

pub mod backend;
pub mod model;
pub mod mpris;
pub mod service;

pub use backend::{MediaBackend, MediaBackendEvent, MediaBackendRef};
pub use model::{MediaModel, PlaybackStatus};
pub use mpris::{
    MprisBackend, MPRIS_PATH, MPRIS_PREFIX, PLAYER_INTERFACE, PROPERTIES_INTERFACE, ROOT_INTERFACE,
};
pub use service::{MediaService, MediaServiceConfig};
