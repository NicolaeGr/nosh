//! Why a value changed.
//!
//! Audio and brightness share the type. The UI shows the change indicator only
//! for [`ChangeOrigin::User`]. `Restore` marks a value written back from a saved
//! brightness snapshot, and `Automatic` marks a change the shell made without a
//! user gesture, such as idle dimming.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeOrigin {
    /// The shell sent the command.
    User,
    /// Another application, keybind, or device changed the value.
    External,
    /// A saved brightness snapshot was restored.
    Restore,
    /// The shell changed the value without a user gesture.
    Automatic,
}
