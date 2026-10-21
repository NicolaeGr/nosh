use std::ffi::OsString;

use thiserror::Error;

/// A failure to start the core runtime.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("failed to start the core runtime: {0}")]
    Runtime(#[from] std::io::Error),
}

/// A failure to resolve an XDG path.
#[derive(Debug, Error)]
pub enum PathError {
    #[error("cannot determine the user home directory: HOME is not set")]
    NoHome,
    #[error("HYPRLAND_INSTANCE_SIGNATURE is set but XDG_RUNTIME_DIR is not")]
    NoRuntimeDir,
    #[error("{name} is not valid UTF-8: {value:?}")]
    NonUtf8 { name: &'static str, value: OsString },
}

/// A backend failed to probe, read, or write.
#[derive(Debug, Clone, Error)]
pub enum BackendError {
    #[error("backend is unavailable: {0}")]
    Unavailable(String),
    #[error("backend {backend} does not support {capability}")]
    Unsupported {
        backend: &'static str,
        capability: &'static str,
    },
    #[error("compositor request failed: {0}")]
    Request(String),
    #[error("compositor returned invalid JSON: {0}")]
    Json(String),
    #[error("no backend satisfies the required capabilities")]
    NoBackend,
}

/// A Hyprland event line that does not match the documented payload shape.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventParseError {
    #[error("event {event} expected {expected} fields, got {got}")]
    Fields {
        event: String,
        expected: usize,
        got: usize,
    },
    #[error("event {event} has an invalid value: {value}")]
    Invalid { event: String, value: String },
}
