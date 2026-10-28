//! Authentication requests and results.
//!
//! The password never leaves the process and is wiped when the last copy of
//! the [`Secret`] is dropped.

use std::fmt;

/// A password buffer that is zeroed on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// One password check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthRequest {
    pub username: String,
    pub password: Secret,
}

/// The result of a password check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthResult {
    /// The value the UI sent with the request. The UI uses it to ignore a late
    /// reply for a dialog that has already closed.
    pub request_id: u64,
    /// The failure message, or `None` on success.
    pub error: Option<String>,
}

impl AuthResult {
    pub fn success(request_id: u64) -> Self {
        Self {
            request_id,
            error: None,
        }
    }

    pub fn failed(request_id: u64, message: impl Into<String>) -> Self {
        Self {
            request_id,
            error: Some(message.into()),
        }
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_appears_in_debug_output() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    }

    #[test]
    fn secrets_compare_by_content() {
        assert_eq!(Secret::new("a"), Secret::new("a"));
        assert_ne!(Secret::new("a"), Secret::new("b"));
    }

    #[test]
    fn an_empty_secret_is_reported() {
        assert!(Secret::new("").is_empty());
        assert!(!Secret::new("x").is_empty());
    }
}
