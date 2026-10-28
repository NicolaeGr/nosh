//! One implementation of the password check.
//!
//! PAM is the only backend. The trait keeps the service testable without a
//! real PAM stack.

use std::sync::Arc;

use crate::auth::model::AuthRequest;
use crate::auth::pam;
use crate::error::BackendError;

/// A password check provided by a backend.
pub trait AuthBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// The PAM service that will be used, or an error when none can be used.
    fn probe(&self) -> Result<(), BackendError>;

    fn authenticate(&self, request: &AuthRequest) -> Result<(), BackendError>;
}

pub type AuthBackendRef = Arc<dyn AuthBackend>;

/// The PAM backend.
pub struct PamBackend {
    service: Option<String>,
}

impl PamBackend {
    /// Resolves the service stack. The configured service wins. When it is
    /// missing and the fallback is allowed, the `login` stack is used with a
    /// warning, because that stack can grant more than a shell-specific one.
    pub fn new(service: &str, allow_fallback: bool) -> Self {
        let resolved = if pam::service_exists(service) {
            Some(service.to_string())
        } else if allow_fallback && pam::service_exists("login") {
            tracing::warn!(
                service,
                "the PAM service file is missing, falling back to `login`"
            );
            Some("login".to_string())
        } else {
            tracing::warn!(service, "no usable PAM service was found");
            None
        };

        Self { service: resolved }
    }

    pub fn service(&self) -> Option<&str> {
        self.service.as_deref()
    }
}

impl AuthBackend for PamBackend {
    fn name(&self) -> &'static str {
        "pam"
    }

    fn probe(&self) -> Result<(), BackendError> {
        if !pam::available() {
            return Err(BackendError::Unavailable(
                "libpam is not installed".to_string(),
            ));
        }
        if self.service.is_none() {
            return Err(BackendError::Unavailable(
                "no PAM service file is installed".to_string(),
            ));
        }
        Ok(())
    }

    fn authenticate(&self, request: &AuthRequest) -> Result<(), BackendError> {
        let service = self.service.as_deref().ok_or_else(|| {
            BackendError::Unavailable("no PAM service file is installed".to_string())
        })?;

        pam::authenticate(service, &request.username, request.password.as_bytes())
            .map_err(BackendError::Message)
    }
}
