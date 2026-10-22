//! One implementation of stats sampling.
//!
//! `/proc` is the only source. The trait keeps the service testable without a
//! real `/proc` tree.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::StatsError;
use crate::stats::model::{CpuTimes, MemoryInfo};

/// Reads CPU counters and memory totals.
#[async_trait]
pub trait StatsSource: Send + Sync {
    fn name(&self) -> &'static str;

    async fn read_cpu(&self) -> Result<CpuTimes, StatsError>;

    async fn read_memory(&self) -> Result<MemoryInfo, StatsError>;
}

pub type StatsSourceRef = Arc<dyn StatsSource>;
