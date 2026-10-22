//! Process helpers shared by the brightness backends.
//!
//! DDC/CI calls run on a blocking task, so this module offers a blocking runner
//! with a timeout as well as the async runner used by the internal fallbacks.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::BrightnessError;

/// Whether a binary can be found on `PATH`.
pub fn command_exists(binary: &str) -> bool {
    if binary.contains('/') {
        return std::path::Path::new(binary).is_file();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(binary).is_file())
}

/// Runs a command with a timeout. Returns stdout, or an error that includes
/// stderr when the process exits unsuccessfully.
pub fn run_blocking(
    binary: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, BrightnessError> {
    let mut child = Command::new(binary)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| BrightnessError::Unavailable(format!("cannot run {binary}: {error}")))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                if status.success() {
                    return Ok(stdout);
                }
                let message = stderr.trim();
                return Err(BrightnessError::Command(if message.is_empty() {
                    format!("{binary} exited with {status}")
                } else {
                    message.to_string()
                }));
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(BrightnessError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                return Err(BrightnessError::Unavailable(format!(
                    "cannot wait for {binary}: {error}"
                )));
            }
        }
    }
}

/// Async variant used by the command and brightnessctl backends.
pub async fn run_async(
    binary: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, BrightnessError> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);

    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| BrightnessError::Timeout)?
        .map_err(|error| BrightnessError::Unavailable(format!("cannot run {binary}: {error}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(BrightnessError::Command(if stderr.is_empty() {
            format!("{binary} exited with {}", output.status)
        } else {
            stderr
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_is_on_the_path() {
        assert!(command_exists("sh"));
    }

    #[test]
    fn a_missing_binary_is_not_on_the_path() {
        assert!(!command_exists("nosh-test-binary-that-does-not-exist"));
    }

    #[test]
    fn a_blocking_command_returns_stdout() {
        let output = run_blocking("sh", &["-c", "printf 42"], Duration::from_secs(2)).unwrap();
        assert_eq!(output, "42");
    }

    #[test]
    fn a_blocking_timeout_is_reported() {
        let error = run_blocking("sh", &["-c", "sleep 2"], Duration::from_millis(100)).unwrap_err();
        assert_eq!(error, BrightnessError::Timeout);
    }
}
