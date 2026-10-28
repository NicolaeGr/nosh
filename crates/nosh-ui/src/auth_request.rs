//! Authentication request ids.
//!
//! The power menu and the lock screen both talk to `AuthService`, and a late
//! reply must never be mistaken for a request from the other window. One
//! process-wide counter keeps their ids apart.

use std::sync::atomic::{AtomicU64, Ordering};

use nosh_core::Secret;

static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn next_request_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Copies typed text into a [`Secret`].
///
/// A `GString` turns into `Vec<u8>` with its trailing NUL (`into_bytes_with_nul`),
/// which PAM and the polkit helper reject as an invalid character. Copying the
/// `str` bytes keeps the password exact.
pub fn secret_from_text(text: &str) -> Secret {
    Secret::new(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::glib;

    #[test]
    fn a_secret_copies_the_text_without_a_trailing_nul() {
        let text = glib::GString::from("hunter2");
        assert_eq!(secret_from_text(&text).as_bytes(), b"hunter2");
    }
}
