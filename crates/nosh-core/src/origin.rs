//! Why a value changed.
//!
//! The change indicator shows only [`ChangeOrigin::User`]. Audio and brightness
//! share the type so the UI handles both services the same way. Brightness adds
//! [`ChangeOrigin::Restore`] so a restore does not flash the indicator.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeOrigin {
    /// The shell sent the command.
    User,
    /// Another application, keybind, or device changed the value.
    External,
    /// The first snapshot after startup or a reconnect.
    Initial,
    /// A saved brightness snapshot was restored.
    Restore,
}
