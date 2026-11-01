//! Helpers shared by the long-running services.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_channel::Receiver;

use crate::command::Command;

/// A time-based seed for [`crate::backoff::Backoff`].
pub(crate) fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

/// Waits for `duration` and drops any command that arrives meanwhile.
///
/// A service that is reconnecting cannot act on commands. Returns true when the
/// command channel closed, which means the shell is shutting down.
pub(crate) async fn wait_or_closed(commands: &Receiver<Command>, duration: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => false,
        result = commands.recv() => {
            if result.is_ok() {
                tracing::debug!("dropping a command while the service reconnects");
            }
            result.is_err()
        }
    }
}

/// Cancels a task and waits for it to stop, so its resources are released
/// before the service returns.
///
/// A handle whose output was already taken by `select!` must not be polled
/// again, so a finished task is skipped.
pub(crate) async fn stop_task<T>(task: &mut tokio::task::JoinHandle<T>) {
    if task.is_finished() {
        return;
    }
    task.abort();
    let _ = task.await;
}
