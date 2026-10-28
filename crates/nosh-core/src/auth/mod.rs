//! PAM authentication for the power menu and future prompts.

pub mod backend;
pub mod model;
pub mod pam;
pub mod service;

pub use backend::{AuthBackend, AuthBackendRef, PamBackend};
pub use model::{AuthRequest, AuthResult, Secret};
pub use service::{AuthService, AuthServiceConfig};
