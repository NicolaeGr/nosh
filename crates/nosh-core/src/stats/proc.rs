//! Reads `/proc/stat` and `/proc/meminfo`.
//!
//! The parsers take text so they can be tested without a real proc tree. The
//! source reads files under a configurable root, which defaults to `/proc`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::StatsError;
use crate::stats::backend::StatsSource;
use crate::stats::model::{CpuTimes, MemoryInfo};

pub const STAT_PATH: &str = "/proc/stat";
pub const MEMINFO_PATH: &str = "/proc/meminfo";

/// Reads the two proc files the stats service needs.
pub struct ProcStatsSource {
    root: PathBuf,
}

impl ProcStatsSource {
    pub fn new() -> Self {
        Self {
            root: PathBuf::from("/proc"),
        }
    }

    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Default for ProcStatsSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatsSource for ProcStatsSource {
    fn name(&self) -> &'static str {
        "proc"
    }

    async fn read_cpu(&self) -> Result<CpuTimes, StatsError> {
        let path = self.root.join("stat");
        let text = read_text(&path).await?;
        parse_cpu_stat(&text).map_err(|message| StatsError::Parse {
            path: path.display().to_string(),
            message,
        })
    }

    async fn read_memory(&self) -> Result<MemoryInfo, StatsError> {
        let path = self.root.join("meminfo");
        let text = read_text(&path).await?;
        parse_meminfo(&text).map_err(|message| StatsError::Parse {
            path: path.display().to_string(),
            message,
        })
    }
}

async fn read_text(path: &Path) -> Result<String, StatsError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| StatsError::Read {
            path: path.display().to_string(),
            message: error.to_string(),
        })
}

/// Parses the aggregate `cpu` line.
///
/// The fields after `cpu` are `user nice system idle iowait irq softirq steal
/// guest guest_nice`. The total is the sum of every field and the idle share is
/// `idle + iowait`, as the feature spec requires.
pub fn parse_cpu_stat(text: &str) -> Result<CpuTimes, String> {
    let line = text
        .lines()
        .find(|line| line.split_whitespace().next() == Some("cpu"))
        .ok_or_else(|| "the aggregate cpu line is missing".to_string())?;

    let fields = line
        .split_whitespace()
        .skip(1)
        .map(|field| {
            field
                .parse::<u64>()
                .map_err(|_| format!("invalid cpu counter {field:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if fields.len() < 4 {
        return Err(format!(
            "the cpu line has {} counters, at least 4 are required",
            fields.len()
        ));
    }

    let total = fields.iter().copied().fold(0u64, u64::saturating_add);
    let idle = fields[3].saturating_add(fields.get(4).copied().unwrap_or(0));

    Ok(CpuTimes { total, idle })
}

/// Parses `MemTotal` and `MemAvailable`.
///
/// When `MemAvailable` is missing, the available value is `MemFree + Buffers +
/// Cached`.
pub fn parse_meminfo(text: &str) -> Result<MemoryInfo, String> {
    let mut total = None;
    let mut available = None;
    let mut free = None;
    let mut buffers = None;
    let mut cached = None;

    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(value) = rest.split_whitespace().next() else {
            continue;
        };
        let Ok(value) = value.parse::<u64>() else {
            continue;
        };

        match name.trim() {
            "MemTotal" => total = Some(value),
            "MemAvailable" => available = Some(value),
            "MemFree" => free = Some(value),
            "Buffers" => buffers = Some(value),
            "Cached" => cached = Some(value),
            _ => {}
        }
    }

    let total = total.ok_or_else(|| "MemTotal is missing".to_string())?;
    let available = match available {
        Some(value) => value,
        None => free
            .ok_or_else(|| "MemAvailable and MemFree are missing".to_string())?
            .saturating_add(buffers.unwrap_or(0))
            .saturating_add(cached.unwrap_or(0)),
    };

    Ok(MemoryInfo {
        total_kib: total,
        available_kib: available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aggregate_cpu_line_is_parsed() {
        let text = "\
cpu  10 5 3 70 6 1 2 3 0 0
cpu0 5 2 1 35 3 0 1 1 0 0
";
        assert_eq!(
            parse_cpu_stat(text),
            Ok(CpuTimes {
                total: 100,
                idle: 76,
            })
        );
    }

    #[test]
    fn a_short_cpu_line_is_rejected() {
        let text = "cpu  10 5 3\n";
        assert!(parse_cpu_stat(text).is_err());
    }

    #[test]
    fn a_missing_cpu_line_is_rejected() {
        let text = "cpu0 1 2 3 4\nintr 1 2 3\n";
        assert!(parse_cpu_stat(text).is_err());
    }

    #[test]
    fn a_non_numeric_counter_is_rejected() {
        let text = "cpu  10 5 x 70 6 1 2 3 0 0\n";
        assert!(parse_cpu_stat(text).is_err());
    }

    #[test]
    fn meminfo_uses_mem_available() {
        let text = "\
MemTotal:       16000000 kB
MemFree:         2000000 kB
MemAvailable:    6000000 kB
Buffers:          100000 kB
Cached:          4000000 kB
";
        assert_eq!(
            parse_meminfo(text),
            Ok(MemoryInfo {
                total_kib: 16_000_000,
                available_kib: 6_000_000,
            })
        );
    }

    #[test]
    fn meminfo_falls_back_to_free_buffers_and_cached() {
        let text = "\
MemTotal:       16000000 kB
MemFree:         2000000 kB
Buffers:          100000 kB
Cached:          4000000 kB
";
        assert_eq!(
            parse_meminfo(text),
            Ok(MemoryInfo {
                total_kib: 16_000_000,
                available_kib: 6_100_000,
            })
        );
    }

    #[test]
    fn meminfo_requires_a_total() {
        let text = "MemFree: 2000000 kB\n";
        assert!(parse_meminfo(text).is_err());
    }

    #[test]
    fn meminfo_requires_available_or_free() {
        let text = "MemTotal: 16000000 kB\nBuffers: 100 kB\nCached: 200 kB\n";
        assert!(parse_meminfo(text).is_err());
    }
}
