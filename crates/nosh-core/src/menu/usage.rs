//! Launch history used to order the `apps` builtin.
//!
//! Each app keeps a short log of launch times. The score sums an exponential
//! decay over those times, so a handful of recent launches outweighs months of
//! steady use. A plain frequency counter cannot do that: a new favourite would
//! need thousands of launches to pass an old one.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The number of launches kept per app. Older launches fall off the end.
const LOG_LEN: usize = 12;

/// A launch loses half its weight every three days.
const HALF_LIFE_SECONDS: f64 = 3.0 * 86_400.0;

/// Launches older than this are dropped from the file.
const MAX_AGE_SECONDS: i64 = 90 * 86_400;

/// The schema version of the usage file.
const FILE_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct UsageFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    apps: BTreeMap<String, Vec<i64>>,
}

/// Launch history for every app, shared by the service and the source.
///
/// `path` is `None` when the store is in-memory only, as in tests.
#[derive(Debug)]
pub struct Usage {
    path: Option<PathBuf>,
    apps: RwLock<BTreeMap<String, Vec<i64>>>,
}

impl Usage {
    /// Loads the usage file. A missing or unreadable file starts an empty
    /// store, because ranking is not important enough to stop the launcher.
    pub fn load(path: PathBuf, now: i64) -> Self {
        let apps = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<UsageFile>(&text) {
                Ok(file) if file.version == FILE_VERSION => prune(file.apps, now),
                Ok(file) => {
                    tracing::warn!(
                        version = file.version,
                        path = %path.display(),
                        "ignoring the usage file with an unknown version"
                    );
                    BTreeMap::new()
                }
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "the usage file is invalid");
                    BTreeMap::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot read the usage file");
                BTreeMap::new()
            }
        };

        Self {
            path: Some(path),
            apps: RwLock::new(apps),
        }
    }

    /// An in-memory store, used by tests and when no path is known.
    pub fn empty() -> Self {
        Self {
            path: None,
            apps: RwLock::new(BTreeMap::new()),
        }
    }

    /// Records one launch and writes the file back.
    pub fn record(&self, id: &str, now: i64) {
        if id.is_empty() {
            return;
        }

        {
            let Ok(mut apps) = self.apps.write() else {
                return;
            };
            let times = apps.entry(id.to_string()).or_default();
            times.push(now);
            times.sort_unstable();
            let excess = times.len().saturating_sub(LOG_LEN);
            if excess > 0 {
                times.drain(..excess);
            }
        }

        self.persist();
    }

    /// The decayed launch score. Zero means the app has no usable history.
    pub fn score(&self, id: &str, now: i64) -> f64 {
        let Ok(apps) = self.apps.read() else {
            return 0.0;
        };
        let Some(times) = apps.get(id) else {
            return 0.0;
        };
        score_times(times, now)
    }

    fn persist(&self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };

        let Ok(apps) = self.apps.read() else {
            return;
        };
        let file = UsageFile {
            version: FILE_VERSION,
            apps: apps.clone(),
        };
        drop(apps);

        let Some(parent) = path.parent() else {
            return;
        };
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(%error, path = %parent.display(), "cannot create the data directory");
            return;
        }

        let text = match serde_json::to_string(&file) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(%error, "cannot serialize the usage file");
                return;
            }
        };

        let temporary = path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(&temporary, text) {
            tracing::warn!(%error, path = %temporary.display(), "cannot write the usage file");
            return;
        }
        if let Err(error) = std::fs::rename(&temporary, path) {
            tracing::warn!(
                %error,
                path = %path.display(),
                "cannot replace the usage file"
            );
        }
    }
}

fn score_times(times: &[i64], now: i64) -> f64 {
    times
        .iter()
        .map(|&time| {
            let age = (now - time).max(0) as f64;
            0.5f64.powf(age / HALF_LIFE_SECONDS)
        })
        .sum()
}

fn prune(mut apps: BTreeMap<String, Vec<i64>>, now: i64) -> BTreeMap<String, Vec<i64>> {
    apps.retain(|_, times| {
        times.retain(|&time| now - time <= MAX_AGE_SECONDS);
        !times.is_empty()
    });
    apps
}

/// Unix seconds. Clock skew backwards yields zero rather than a negative time.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_700_000_000;

    fn usage_with(entries: &[(&str, &[i64])]) -> Usage {
        let usage = Usage::empty();
        {
            let mut apps = usage.apps.write().unwrap();
            for (id, times) in entries {
                apps.insert(id.to_string(), times.to_vec());
            }
        }
        usage
    }

    #[test]
    fn a_few_recent_launches_beat_a_long_old_habit() {
        let steady: Vec<i64> = (0..12).map(|day| NOW - day * DAY).collect();
        let new_favourite: Vec<i64> = vec![NOW - 60, NOW - 50, NOW - 40, NOW - 30, NOW - 20];

        let usage = usage_with(&[("steady", &steady), ("new", &new_favourite)]);
        assert!(usage.score("new", NOW) > usage.score("steady", NOW));
    }

    #[test]
    fn three_recent_launches_do_not_beat_a_daily_habit() {
        let steady: Vec<i64> = (0..12).map(|day| NOW - day * DAY).collect();
        let newcomer: Vec<i64> = vec![NOW - 60, NOW - 50, NOW - 40];

        let usage = usage_with(&[("steady", &steady), ("newcomer", &newcomer)]);
        assert!(usage.score("steady", NOW) > usage.score("newcomer", NOW));
    }

    #[test]
    fn old_launches_decay_to_nothing() {
        let usage = usage_with(&[("gone", &[NOW - 60 * DAY])]);
        assert!(usage.score("gone", NOW) < 0.01);
        assert_eq!(usage.score("missing", NOW), 0.0);
    }

    #[test]
    fn future_timestamps_count_as_now() {
        let usage = usage_with(&[("skewed", &[NOW + DAY])]);
        assert!((usage.score("skewed", NOW) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_log_stays_bounded() {
        let usage = Usage::empty();
        for step in 0..20 {
            usage.record("app", NOW + step);
        }
        let apps = usage.apps.read().unwrap();
        assert_eq!(apps["app"].len(), LOG_LEN);
        assert_eq!(apps["app"].last(), Some(&(NOW + 19)));
    }

    #[test]
    fn records_round_trip_through_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nosh/app_usage.json");

        let usage = Usage::load(path.clone(), NOW);
        usage.record("firefox", NOW - 10);
        usage.record("firefox", NOW - 5);
        usage.record("firefox", NOW);

        let reloaded = Usage::load(path, NOW);
        assert!(reloaded.score("firefox", NOW) > 2.5);
    }

    #[test]
    fn expired_history_is_pruned_on_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app_usage.json");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"apps":{{"old":[{}],"new":[{}]}}}}"#,
                NOW - MAX_AGE_SECONDS - 1,
                NOW - 10
            ),
        )
        .unwrap();

        let usage = Usage::load(path, NOW);
        assert_eq!(usage.score("old", NOW), 0.0);
        assert!(usage.score("new", NOW) > 0.0);
    }

    #[test]
    fn an_invalid_file_starts_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app_usage.json");
        std::fs::write(&path, "not json").unwrap();

        let usage = Usage::load(path, NOW);
        assert_eq!(usage.score("anything", NOW), 0.0);
    }
}
