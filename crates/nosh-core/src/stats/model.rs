//! CPU and memory percentages as the UI renders them.
//!
//! The counters stay in the core. The UI receives two whole percentages in
//! `0..=100` and maps them to its level classes.

/// Aggregate CPU counters from the `cpu` line in `/proc/stat`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    /// The sum of every counter in the line.
    pub total: u64,
    /// `idle + iowait`.
    pub idle: u64,
}

/// Memory totals from `/proc/meminfo`, in KiB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
}

impl MemoryInfo {
    /// Used memory as a whole percent, rounded to the nearest.
    pub fn percent(&self) -> u8 {
        if self.total_kib == 0 {
            return 0;
        }

        let used = self.total_kib.saturating_sub(self.available_kib);
        let percent =
            (u128::from(used) * 100 + u128::from(self.total_kib) / 2) / u128::from(self.total_kib);
        percent.min(100) as u8
    }
}

/// The CPU and memory percentages the bar renders.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemStats {
    pub cpu_percent: u8,
    pub mem_percent: u8,
}

/// The busy share between two CPU samples.
///
/// Returns `None` for the first sample and when a counter went backwards, which
/// happens after a container restart or a counter wrap. The caller keeps the
/// previous percentage and samples again on the next tick.
pub fn cpu_percent(previous: Option<CpuTimes>, current: CpuTimes) -> Option<u8> {
    let previous = previous?;
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;

    if total == 0 || idle > total {
        return None;
    }

    let busy = 1.0 - (idle as f64 / total as f64);
    Some((busy * 100.0).round().clamp(0.0, 100.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(total: u64, idle: u64) -> CpuTimes {
        CpuTimes { total, idle }
    }

    #[test]
    fn the_first_sample_has_no_percentage() {
        assert_eq!(cpu_percent(None, sample(100, 40)), None);
    }

    #[test]
    fn cpu_percent_uses_the_counter_delta() {
        assert_eq!(
            cpu_percent(Some(sample(100, 40)), sample(200, 60)),
            Some(80)
        );
        assert_eq!(
            cpu_percent(Some(sample(100, 40)), sample(300, 150)),
            Some(45)
        );
    }

    #[test]
    fn cpu_percent_rounds_to_the_nearest_whole_percent() {
        // 100 total, 49 idle -> 51 percent busy.
        assert_eq!(cpu_percent(Some(sample(0, 0)), sample(100, 49)), Some(51));
        // 100 total, 50.5 idle -> 49.5 percent busy.
        assert_eq!(cpu_percent(Some(sample(0, 0)), sample(200, 101)), Some(50));
    }

    #[test]
    fn a_zero_total_delta_keeps_the_previous_value() {
        assert_eq!(cpu_percent(Some(sample(100, 40)), sample(100, 40)), None);
    }

    #[test]
    fn a_counter_reset_keeps_the_previous_value() {
        assert_eq!(cpu_percent(Some(sample(100, 40)), sample(50, 20)), None);
        assert_eq!(cpu_percent(Some(sample(100, 40)), sample(200, 10)), None);
    }

    #[test]
    fn memory_percent_uses_total_and_available() {
        let info = MemoryInfo {
            total_kib: 1000,
            available_kib: 250,
        };
        assert_eq!(info.percent(), 75);
    }

    #[test]
    fn memory_percent_rounds_to_the_nearest() {
        assert_eq!(
            MemoryInfo {
                total_kib: 200,
                available_kib: 199,
            }
            .percent(),
            1
        );
        assert_eq!(
            MemoryInfo {
                total_kib: 3,
                available_kib: 2,
            }
            .percent(),
            33
        );
    }

    #[test]
    fn memory_percent_clamps_and_handles_an_empty_total() {
        assert_eq!(
            MemoryInfo {
                total_kib: 0,
                available_kib: 0,
            }
            .percent(),
            0
        );
        assert_eq!(
            MemoryInfo {
                total_kib: 100,
                available_kib: 500,
            }
            .percent(),
            0
        );
    }
}
