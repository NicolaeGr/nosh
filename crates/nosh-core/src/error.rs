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
    /// A user-facing message that needs no backend prefix.
    #[error("{0}")]
    Message(String),
}

/// A monitor layout operation failed. The messages are short and safe to show
/// to the user. A missing monitor never stops the service.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MonitorError {
    #[error("monitor {0:?} was not found")]
    TargetNotFound(String),
    #[error("no monitor layout is available")]
    NoLayout,
    #[error("no monitor was specified and there is no primary")]
    NoTarget,
    #[error("the monitor backend is read-only")]
    ReadOnly,
    #[error("monitor {0} needs a resolution before it can be enabled")]
    MissingResolution(String),
    #[error("no enabled monitor can be used as a mirror source")]
    NoMirrorSource,
    #[error("unknown monitor placement policy: {0}")]
    UnknownPolicy(String),
    #[error("unknown layout preset: {0}")]
    UnknownPreset(String),
    #[error("monitor write failed: {0}")]
    Write(String),
}

/// A brightness device failed to probe, read, or write.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BrightnessError {
    #[error("brightness backend is unavailable: {0}")]
    Unavailable(String),
    #[error("brightness backend does not support {capability} for {device}")]
    Unsupported {
        capability: &'static str,
        device: String,
    },
    #[error("brightness device {0} was not found")]
    NotFound(String),
    #[error("brightness command failed: {0}")]
    Command(String),
    #[error("cannot parse brightness output: {0}")]
    Parse(String),
    #[error("brightness operation timed out")]
    Timeout,
    #[error("brightness snapshot failed: {0}")]
    Snapshot(String),
}

/// A system stats read or parse failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StatsError {
    #[error("cannot read {path}: {message}")]
    Read { path: String, message: String },
    #[error("cannot parse {path}: {message}")]
    Parse { path: String, message: String },
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
