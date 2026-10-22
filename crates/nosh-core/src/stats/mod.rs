//! System stats from `/proc`.
//!
//! The core owns CPU and memory sampling. The UI receives whole percentages and
//! maps them to level classes.

pub mod backend;
pub mod model;
pub mod proc;
pub mod service;

pub use crate::event::SystemStatsEvent;
pub use backend::{StatsSource, StatsSourceRef};
pub use model::{cpu_percent, CpuTimes, MemoryInfo, SystemStats};
pub use proc::{parse_cpu_stat, parse_meminfo, ProcStatsSource, MEMINFO_PATH, STAT_PATH};
pub use service::{SystemStatsService, SystemStatsServiceConfig, DEFAULT_INTERVAL};
