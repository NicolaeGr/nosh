//! User-configured command backends.
//!
//! A command device covers a monitor DDC cannot reach or a compositor-specific
//! brightness tool. Only an integer percentage is substituted into the set
//! command.

use std::time::Duration;

use async_trait::async_trait;

use crate::brightness::backend::{BrightnessBackend, BrightnessCapabilities};
use crate::brightness::model::{
    clamp_percent, BrightnessDeviceId, BrightnessDeviceInfo, BrightnessReading,
};
use crate::brightness::process::{command_exists, run_async};
use crate::error::BrightnessError;

/// One `[[brightness.device]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDeviceConfig {
    pub name: String,
    pub get: String,
    pub set: String,
}

impl CommandDeviceConfig {
    pub fn new(name: impl Into<String>, get: impl Into<String>, set: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            get: get.into(),
            set: set.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandBackend {
    devices: Vec<CommandDeviceConfig>,
    shell: String,
    timeout: Duration,
}

impl CommandBackend {
    pub fn new(devices: Vec<CommandDeviceConfig>) -> Self {
        Self::with_config(devices, "sh", Duration::from_secs(3))
    }

    pub fn with_config(devices: Vec<CommandDeviceConfig>, shell: &str, timeout: Duration) -> Self {
        Self {
            devices,
            shell: shell.to_string(),
            timeout,
        }
    }

    fn config(&self, name: &str) -> Option<&CommandDeviceConfig> {
        self.devices.iter().find(|device| device.name == name)
    }

    async fn run(&self, script: &str) -> Result<String, BrightnessError> {
        if !command_exists(&self.shell) {
            return Err(BrightnessError::Unavailable(format!(
                "{} is not installed",
                self.shell
            )));
        }
        run_async(&self.shell, &["-c", script], self.timeout).await
    }
}

#[async_trait]
impl BrightnessBackend for CommandBackend {
    fn name(&self) -> &'static str {
        "command"
    }

    fn supports(&self, device: &BrightnessDeviceId) -> bool {
        match device {
            BrightnessDeviceId::Named(name) => self.config(name).is_some(),
            _ => false,
        }
    }

    async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError> {
        if self.devices.is_empty() {
            return Err(BrightnessError::Unavailable(
                "no brightness command devices are configured".to_string(),
            ));
        }
        if !command_exists(&self.shell) {
            return Err(BrightnessError::Unavailable(format!(
                "{} is not installed",
                self.shell
            )));
        }
        Ok(BrightnessCapabilities::CONTROL)
    }

    async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError> {
        Ok(self
            .devices
            .iter()
            .map(|device| BrightnessDeviceInfo::command(device.name.clone()))
            .collect())
    }

    async fn read(
        &self,
        device: &BrightnessDeviceId,
    ) -> Result<BrightnessReading, BrightnessError> {
        let BrightnessDeviceId::Named(name) = device else {
            return Err(BrightnessError::Unsupported {
                capability: "read",
                device: device.stable_id(),
            });
        };
        let config = self
            .config(name)
            .ok_or_else(|| BrightnessError::NotFound(name.clone()))?;
        let output = self.run(&config.get).await?;
        let percent = parse_percent_output(&output).ok_or_else(|| {
            BrightnessError::Parse(format!("unexpected command output {output:?}"))
        })?;
        Ok(BrightnessReading::percent(percent))
    }

    async fn write(
        &self,
        device: &BrightnessDeviceId,
        percent: f64,
    ) -> Result<(), BrightnessError> {
        let BrightnessDeviceId::Named(name) = device else {
            return Err(BrightnessError::Unsupported {
                capability: "write",
                device: device.stable_id(),
            });
        };
        let config = self
            .config(name)
            .ok_or_else(|| BrightnessError::NotFound(name.clone()))?;
        let script = substitute_percent(&config.set, percent);
        self.run(&script).await?;
        Ok(())
    }
}

/// Replaces `{value}` with a rounded integer percentage.
pub fn substitute_percent(template: &str, percent: f64) -> String {
    let value = clamp_percent(percent).round() as i64;
    template.replace("{value}", &value.to_string())
}

/// Reads the first number in command output. `light -G` prints `50.00`, and
/// some scripts print `50%`.
pub fn parse_percent_output(output: &str) -> Option<f64> {
    for token in output.split_whitespace() {
        let token = token.trim_end_matches('%');
        if let Ok(value) = token.parse::<f64>() {
            if value.is_finite() {
                return Some(clamp_percent(value));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_placeholder_becomes_an_integer() {
        assert_eq!(substitute_percent("light -S {value}", 42.6), "light -S 43");
        assert_eq!(substitute_percent("light -S {value}", -5.0), "light -S 0");
        assert_eq!(
            substitute_percent("light -S {value}", 500.0),
            "light -S 100"
        );
    }

    #[test]
    fn output_parsing_reads_a_plain_number() {
        assert_eq!(parse_percent_output("50.00\n"), Some(50.0));
        assert_eq!(parse_percent_output("brightness: 42%\n"), Some(42.0));
        assert_eq!(parse_percent_output("nothing here"), None);
    }
}
