//! The lock intent marker.
//!
//! `ext-session-lock-v1` keeps the session locked when the client dies, but the
//! compositor never tells a restarted client that a lock is still active, and
//! Hyprland does not update logind's `LockedHint` either. nosh writes a marker
//! in the runtime directory before it asks for the lock, so a restart can
//! reacquire it. The runtime directory is cleared at logout, which keeps a
//! stale marker from locking a session that was never locked.

use std::io;
use std::path::{Path, PathBuf};

use crate::paths::NoshPaths;

pub const LOCK_MARKER_FILE: &str = "session-lock";

/// One marker file that records a lock the shell intends to hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockMarker {
    path: PathBuf,
}

impl LockMarker {
    /// Resolves the marker under `$XDG_RUNTIME_DIR/nosh`.
    ///
    /// Returns `None` when the runtime directory is unknown, because a marker
    /// in the home directory would outlive the session it describes.
    pub fn from_env() -> Option<Self> {
        match NoshPaths::from_env() {
            Ok(paths) => Self::from_paths(&paths),
            Err(error) => {
                tracing::warn!(%error, "the lock marker path could not be resolved");
                None
            }
        }
    }

    pub fn from_paths(paths: &NoshPaths) -> Option<Self> {
        paths
            .runtime_dir
            .as_ref()
            .map(|directory| Self::at(directory.join(LOCK_MARKER_FILE)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Records that a lock is wanted. The parent directory is created when the
    /// session has not written anything under the runtime directory yet.
    pub fn set(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, b"")
    }

    /// Removes the marker. A missing file is not an error, so every teardown
    /// path can call this.
    pub fn clear(&self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    %error,
                    path = %self.path.display(),
                    "the lock marker could not be removed"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use super::*;

    fn paths(runtime: Option<&str>) -> NoshPaths {
        let mut values: HashMap<String, OsString> = HashMap::new();
        values.insert("HOME".to_string(), OsString::from("/home/nicolae"));
        if let Some(runtime) = runtime {
            values.insert("XDG_RUNTIME_DIR".to_string(), OsString::from(runtime));
        }
        NoshPaths::from_lookup(|key| values.get(key).cloned()).unwrap()
    }

    #[test]
    fn the_marker_lives_in_the_runtime_directory() {
        let marker = LockMarker::from_paths(&paths(Some("/run/user/1000"))).unwrap();
        assert_eq!(marker.path(), Path::new("/run/user/1000/nosh/session-lock"));
    }

    #[test]
    fn a_missing_runtime_directory_disables_the_marker() {
        assert_eq!(LockMarker::from_paths(&paths(None)), None);
    }

    #[test]
    fn set_and_clear_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let marker = LockMarker::at(directory.path().join("nosh/session-lock"));

        assert!(!marker.exists());
        marker.set().unwrap();
        assert!(marker.exists());
        marker.clear();
        assert!(!marker.exists());
        // Clearing twice is not an error.
        marker.clear();
        assert!(!marker.exists());
    }
}
