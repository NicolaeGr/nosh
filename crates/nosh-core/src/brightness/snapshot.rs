//! Atomic persistence for the brightness snapshot.

use std::path::Path;

use tokio::io::AsyncWriteExt;

use crate::brightness::model::BrightnessSnapshotFile;
use crate::error::BrightnessError;

/// Reads a snapshot. A missing file is `None`, not an error.
pub async fn load(path: &Path) -> Result<Option<BrightnessSnapshotFile>, BrightnessError> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| {
            BrightnessError::Snapshot(format!("cannot parse {}: {error}", path.display()))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BrightnessError::Snapshot(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

/// Writes a snapshot through a temporary file in the same directory and renames
/// it over the destination.
pub async fn save(path: &Path, snapshot: &BrightnessSnapshotFile) -> Result<(), BrightnessError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            BrightnessError::Snapshot(format!("cannot create {}: {error}", parent.display()))
        })?;
    }

    let json = serde_json::to_vec_pretty(snapshot)
        .map_err(|error| BrightnessError::Snapshot(format!("cannot encode snapshot: {error}")))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot.json");
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));

    let mut file = tokio::fs::File::create(&temporary).await.map_err(|error| {
        BrightnessError::Snapshot(format!("cannot create {}: {error}", temporary.display()))
    })?;
    file.write_all(&json).await.map_err(|error| {
        BrightnessError::Snapshot(format!("cannot write {}: {error}", temporary.display()))
    })?;
    file.sync_all().await.map_err(|error| {
        BrightnessError::Snapshot(format!("cannot sync {}: {error}", temporary.display()))
    })?;
    drop(file);

    tokio::fs::rename(&temporary, path).await.map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        BrightnessError::Snapshot(format!("cannot replace {}: {error}", path.display()))
    })
}

/// Removes a snapshot after a successful restore.
pub async fn remove(path: &Path) -> Result<(), BrightnessError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BrightnessError::Snapshot(format!(
            "cannot remove {}: {error}",
            path.display()
        ))),
    }
}

/// Current UNIX time in seconds, for the snapshot header.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::brightness::model::{BrightnessSnapshotEntry, BrightnessSnapshotFile};

    fn snapshot() -> BrightnessSnapshotFile {
        BrightnessSnapshotFile {
            created: 1_700_000_000,
            devices: vec![BrightnessSnapshotEntry {
                id: "backlight:intel_backlight".to_string(),
                kind: "internal".to_string(),
                percent: 62.0,
                raw: Some(1240),
                bus: None,
                connector: None,
                value: None,
            }],
        }
    }

    #[tokio::test]
    async fn a_snapshot_round_trips_and_creates_directories() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("brightness").join("snapshot.json");
        let original = snapshot();

        save(&path, &original).await.unwrap();
        assert_eq!(load(&path).await.unwrap(), Some(original));
    }

    #[tokio::test]
    async fn a_missing_snapshot_is_none() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("missing.json");
        assert_eq!(load(&path).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_second_save_replaces_the_first_atomically() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        save(&path, &snapshot()).await.unwrap();

        let mut replacement = snapshot();
        replacement.created = 1_800_000_000;
        save(&path, &replacement).await.unwrap();

        let loaded = load(&path).await.unwrap().unwrap();
        assert_eq!(loaded.created, 1_800_000_000);
    }

    #[tokio::test]
    async fn remove_ignores_a_missing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("missing.json");
        remove(&path).await.unwrap();
    }
}
