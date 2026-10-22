//! The system stats service.
//!
//! Samples `/proc/stat` and `/proc/meminfo` on a fixed interval. The first CPU
//! sample establishes the counter baseline and reports 0 percent, so the bar
//! never shows a since-boot average. A failed read leaves the last value in
//! place and logs once per failure streak.

use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};

use crate::command::Command;
use crate::event::{Event, Service, Snapshot, SystemStatsEvent};
use crate::stats::backend::StatsSourceRef;
use crate::stats::model::{cpu_percent, CpuTimes, SystemStats};
use crate::stats::proc::ProcStatsSource;

/// The sampling interval used by the Vala widget.
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(3000);

#[derive(Debug, Clone)]
pub struct SystemStatsServiceConfig {
    pub interval: Duration,
}

impl Default for SystemStatsServiceConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_INTERVAL,
        }
    }
}

pub struct SystemStatsService {
    source: StatsSourceRef,
    config: SystemStatsServiceConfig,
}

impl SystemStatsService {
    pub fn from_env() -> Self {
        Self::from_env_with(SystemStatsServiceConfig::default())
    }

    pub fn from_env_with(config: SystemStatsServiceConfig) -> Self {
        Self::with_source(Arc::new(ProcStatsSource::new()), config)
    }

    pub fn with_source(source: StatsSourceRef, config: SystemStatsServiceConfig) -> Self {
        Self { source, config }
    }

    /// Runs until the command receiver closes or the event receiver is dropped.
    pub async fn run(self, commands: Receiver<Command>, events: Sender<Event>) {
        let mut sampler = Sampler::new(self.source);
        let mut available = None;
        let mut emitted = false;

        loop {
            let (stats, fresh) = sampler.sample().await;

            if fresh {
                let stats = stats.unwrap_or_default();
                if set_availability(&events, &mut available, true)
                    .await
                    .is_err()
                {
                    return;
                }

                if events
                    .send(Event::SystemStats(SystemStatsEvent::Changed(stats)))
                    .await
                    .is_err()
                {
                    return;
                }

                if !emitted {
                    emitted = true;
                    let snapshot = Snapshot {
                        system_stats: Some(stats),
                        ..Snapshot::default()
                    };
                    if events.send(Event::Snapshot(snapshot)).await.is_err() {
                        return;
                    }
                }
            } else if stats.is_none()
                && set_availability(&events, &mut available, false)
                    .await
                    .is_err()
            {
                return;
            }

            if wait_or_closed(&commands, self.config.interval).await {
                return;
            }
        }
    }
}

struct Sampler {
    source: StatsSourceRef,
    previous_cpu: Option<CpuTimes>,
    stats: Option<SystemStats>,
    cpu_failed: bool,
    memory_failed: bool,
}

impl Sampler {
    fn new(source: StatsSourceRef) -> Self {
        Self {
            source,
            previous_cpu: None,
            stats: None,
            cpu_failed: false,
            memory_failed: false,
        }
    }

    /// Reads both sources. The flag is true when at least one read succeeded.
    async fn sample(&mut self) -> (Option<SystemStats>, bool) {
        let mut next = self.stats.unwrap_or_default();
        let mut fresh = false;

        match self.source.read_cpu().await {
            Ok(current) => {
                self.cpu_failed = false;
                fresh = true;
                if let Some(percent) = cpu_percent(self.previous_cpu, current) {
                    next.cpu_percent = percent;
                }
                self.previous_cpu = Some(current);
            }
            Err(error) => {
                if !self.cpu_failed {
                    tracing::warn!(
                        backend = self.source.name(),
                        %error,
                        "a system stats CPU read failed"
                    );
                    self.cpu_failed = true;
                }
            }
        }

        match self.source.read_memory().await {
            Ok(info) => {
                self.memory_failed = false;
                fresh = true;
                next.mem_percent = info.percent();
            }
            Err(error) => {
                if !self.memory_failed {
                    tracing::warn!(
                        backend = self.source.name(),
                        %error,
                        "a system stats memory read failed"
                    );
                    self.memory_failed = true;
                }
            }
        }

        if fresh {
            self.stats = Some(next);
            (Some(next), true)
        } else {
            (self.stats, false)
        }
    }
}

async fn set_availability(
    events: &Sender<Event>,
    available: &mut Option<bool>,
    value: bool,
) -> Result<(), ()> {
    if *available == Some(value) {
        return Ok(());
    }

    events
        .send(Event::ServiceAvailability(Service::SystemStats, value))
        .await
        .map_err(|_| ())?;
    *available = Some(value);
    Ok(())
}

async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a command while the stats service is idle");
            }
            result.is_err()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::error::StatsError;
    use crate::stats::backend::StatsSource;
    use crate::stats::model::MemoryInfo;

    struct FakeSource {
        cpu: Mutex<Vec<CpuTimes>>,
        index: AtomicUsize,
        cpu_error: AtomicBool,
        memory: Mutex<Result<MemoryInfo, StatsError>>,
    }

    impl FakeSource {
        fn new(cpu: Vec<CpuTimes>, memory: MemoryInfo) -> Self {
            Self {
                cpu: Mutex::new(cpu),
                index: AtomicUsize::new(0),
                cpu_error: AtomicBool::new(false),
                memory: Mutex::new(Ok(memory)),
            }
        }

        fn failing() -> Self {
            Self {
                cpu: Mutex::new(Vec::new()),
                index: AtomicUsize::new(0),
                cpu_error: AtomicBool::new(false),
                memory: Mutex::new(Err(read_error())),
            }
        }

        async fn set_cpu(&self, readings: Vec<CpuTimes>) {
            self.index.store(0, Ordering::SeqCst);
            *self.cpu.lock().await = readings;
        }

        async fn set_memory(&self, memory: Result<MemoryInfo, StatsError>) {
            *self.memory.lock().await = memory;
        }

        fn set_cpu_error(&self, failed: bool) {
            self.cpu_error.store(failed, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl StatsSource for FakeSource {
        fn name(&self) -> &'static str {
            "fake"
        }

        async fn read_cpu(&self) -> Result<CpuTimes, StatsError> {
            if self.cpu_error.load(Ordering::SeqCst) {
                return Err(read_error());
            }

            let index = self.index.fetch_add(1, Ordering::SeqCst);
            let readings = self.cpu.lock().await;
            if readings.is_empty() {
                return Err(read_error());
            }
            Ok(readings[index.min(readings.len() - 1)])
        }

        async fn read_memory(&self) -> Result<MemoryInfo, StatsError> {
            self.memory.lock().await.clone()
        }
    }

    fn read_error() -> StatsError {
        StatsError::Read {
            path: "/proc/stat".to_string(),
            message: "test".to_string(),
        }
    }

    fn times(total: u64, idle: u64) -> CpuTimes {
        CpuTimes { total, idle }
    }

    fn memory(total: u64, available: u64) -> MemoryInfo {
        MemoryInfo {
            total_kib: total,
            available_kib: available,
        }
    }

    fn service(source: Arc<FakeSource>) -> SystemStatsService {
        service_at(source, Duration::from_millis(10))
    }

    fn service_at(source: Arc<FakeSource>, interval: Duration) -> SystemStatsService {
        SystemStatsService::with_source(source, SystemStatsServiceConfig { interval })
    }

    fn start(
        service: SystemStatsService,
    ) -> (
        async_channel::Sender<Command>,
        async_channel::Receiver<Event>,
    ) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        tokio::spawn(service.run(command_rx, events));
        (commands, event_rx)
    }

    async fn next_event(events: &async_channel::Receiver<Event>) -> Event {
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("no event arrived")
            .expect("the event channel closed")
    }

    async fn next_stats(events: &async_channel::Receiver<Event>) -> SystemStats {
        loop {
            match next_event(events).await {
                Event::SystemStats(SystemStatsEvent::Changed(stats)) => return stats,
                _ => continue,
            }
        }
    }

    /// Fails when a stats update arrives before the timeout. Other events, such
    /// as the startup snapshot, are ignored.
    async fn expect_no_stats(events: &async_channel::Receiver<Event>, duration: Duration) {
        let deadline = tokio::time::Instant::now() + duration;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return;
            }

            match tokio::time::timeout(remaining, events.recv()).await {
                Err(_) => return,
                Ok(Err(_)) => panic!("the event channel closed"),
                Ok(Ok(Event::SystemStats(_))) => {
                    panic!("a failed read must not emit a stats value")
                }
                Ok(Ok(_)) => continue,
            }
        }
    }

    #[tokio::test]
    async fn startup_reports_availability_then_a_zero_cpu_snapshot() {
        let source = Arc::new(FakeSource::new(vec![times(100, 40)], memory(1000, 250)));
        let (_commands, events) = start(service(source));

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::SystemStats, true)
        ));

        let stats = match next_event(&events).await {
            Event::SystemStats(SystemStatsEvent::Changed(stats)) => stats,
            other => panic!("expected a stats update, got {other:?}"),
        };
        assert_eq!(
            stats,
            SystemStats {
                cpu_percent: 0,
                mem_percent: 75,
            }
        );

        match next_event(&events).await {
            Event::Snapshot(snapshot) => assert_eq!(snapshot.system_stats, Some(stats)),
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_next_sample_uses_cpu_counter_deltas() {
        let source = Arc::new(FakeSource::new(
            vec![times(100, 40), times(200, 60)],
            memory(1000, 250),
        ));
        let (_commands, events) = start(service(source));

        assert_eq!(next_stats(&events).await.cpu_percent, 0);
        let stats = next_stats(&events).await;
        assert_eq!(stats.cpu_percent, 80);
        assert_eq!(stats.mem_percent, 75);
    }

    #[tokio::test]
    async fn a_counter_reset_keeps_the_previous_percentage() {
        let source = Arc::new(FakeSource::new(
            vec![times(100, 40), times(50, 20), times(150, 60)],
            memory(1000, 250),
        ));
        let (_commands, events) = start(service(source));

        assert_eq!(next_stats(&events).await.cpu_percent, 0);
        assert_eq!(next_stats(&events).await.cpu_percent, 0);
        assert_eq!(next_stats(&events).await.cpu_percent, 60);
    }

    #[tokio::test]
    async fn a_read_failure_keeps_the_last_value_and_recovers() {
        let source = Arc::new(FakeSource::new(vec![times(100, 40)], memory(1000, 250)));
        let (_commands, events) = start(service_at(source.clone(), Duration::from_millis(100)));

        let first = next_stats(&events).await;
        assert_eq!(
            first,
            SystemStats {
                cpu_percent: 0,
                mem_percent: 75,
            }
        );

        source.set_cpu_error(true);
        source.set_memory(Err(read_error())).await;
        expect_no_stats(&events, Duration::from_millis(60)).await;

        source.set_cpu_error(false);
        source.set_cpu(vec![times(200, 60)]).await;
        source.set_memory(Ok(memory(1000, 500))).await;

        let recovered = next_stats(&events).await;
        assert_eq!(recovered.cpu_percent, 80);
        assert_eq!(recovered.mem_percent, 50);
    }

    #[tokio::test]
    async fn an_unreadable_proc_tree_reports_unavailable_and_recovers() {
        let source = Arc::new(FakeSource::failing());
        let (commands, events) = start(service(source.clone()));

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::SystemStats, false)
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        expect_no_stats(&events, Duration::from_millis(20)).await;

        source.set_cpu(vec![times(100, 40)]).await;
        source.set_memory(Ok(memory(1000, 250))).await;

        assert!(matches!(
            next_event(&events).await,
            Event::ServiceAvailability(Service::SystemStats, true)
        ));
        assert_eq!(
            next_stats(&events).await,
            SystemStats {
                cpu_percent: 0,
                mem_percent: 75,
            }
        );

        drop(commands);
    }
}
