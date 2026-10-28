//! `brightnessctl` fallback for the internal panel.
//!
//! The service uses it when sysfs plus logind is unavailable. `--exponent=2.0`
//! matches the sysfs curve, and `-m` selects machine-readable output.

use std::time::Duration;

use async_trait::async_trait;

use crate::brightness::backend::{BrightnessBackend, BrightnessCapabilities};
use crate::brightness::model::{
    clamp_percent, round_percent, BrightnessDeviceId, BrightnessDeviceInfo, BrightnessReading,
};
use crate::brightness::process::{command_exists, run_async};
use crate::error::BrightnessError;

#[derive(Debug, Clone)]
pub struct BrightnessctlConfig {
    pub binary: String,
    pub timeout: Duration,
}

impl Default for BrightnessctlConfig {
    fn default() -> Self {
        Self {
            binary: "brightnessctl".to_string(),
            timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrightnessctlBackend {
    config: BrightnessctlConfig,
}

impl BrightnessctlBackend {
    pub fn new() -> Self {
        Self::with_config(BrightnessctlConfig::default())
    }

    pub fn with_config(config: BrightnessctlConfig) -> Self {
        Self { config }
    }

    async fn run(&self, arguments: &[&str]) -> Result<String, BrightnessError> {
        run_async(&self.config.binary, arguments, self.config.timeout).await
    }

    fn device_name<'a>(&self, device: &'a BrightnessDeviceId) -> Result<&'a str, BrightnessError> {
        match device {
            BrightnessDeviceId::Backlight(name) => Ok(name),
            _ => Err(BrightnessError::Unsupported {
                capability: "brightnessctl",
                device: device.stable_id(),
            }),
        }
    }
}

impl Default for BrightnessctlBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BrightnessBackend for BrightnessctlBackend {
    fn name(&self) -> &'static str {
        "brightnessctl"
    }

    fn supports(&self, device: &BrightnessDeviceId) -> bool {
        matches!(device, BrightnessDeviceId::Backlight(_))
    }

    async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError> {
        if !command_exists(&self.config.binary) {
            return Err(BrightnessError::Unavailable(format!(
                "{} is not installed",
                self.config.binary
            )));
        }
        Ok(BrightnessCapabilities::CONTROL)
    }

    async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError> {
        let output = self.run(&["-l", "-m"]).await?;
        Ok(parse_device_list(&output)
            .into_iter()
            .map(BrightnessDeviceInfo::internal)
            .collect())
    }

    async fn read(
        &self,
        device: &BrightnessDeviceId,
    ) -> Result<BrightnessReading, BrightnessError> {
        let name = self.device_name(device)?;
        let output = self.run(&["-d", name, "-m", "--exponent=2.0"]).await?;
        let percent = parse_machine_output(&output).ok_or_else(|| {
            BrightnessError::Parse(format!("unexpected brightnessctl output {output:?}"))
        })?;
        Ok(BrightnessReading {
            percent,
            raw: None,
            max_raw: None,
        })
    }

    async fn write(
        &self,
        device: &BrightnessDeviceId,
        percent: f64,
    ) -> Result<(), BrightnessError> {
        let name = self.device_name(device)?;
        let value = format!("{}%", clamp_percent(percent).round() as i64);
        self.run(&["-d", name, "--exponent=2.0", "set", &value])
            .await?;
        Ok(())
    }
}

/// Parses `brightnessctl -l -m`. Each line is comma separated as
/// `device,class,current,percent%,max`. Only `backlight` class devices are
/// returned; `brightnessctl` also lists keyboard LEDs, which are not displays.
pub fn parse_device_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',');
            let name = fields.next()?.trim();
            let class = fields.next()?.trim();
            if name.is_empty() || class != "backlight" {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

/// Parses the percentage from `brightnessctl -m`.
///
/// The documented machine format is `device,class,current,percent%,max`. Some
/// versions include the device type, so `percent%` is not always at the same
/// index. Only a field with a trailing `%` is a percentage; the trailing `max`
/// field is a raw value and must not be mistaken for one.
pub fn parse_machine_output(output: &str) -> Option<f64> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    line.split(',').rev().find_map(|field| {
        field
            .trim()
            .strip_suffix('%')
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map(round_percent)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_list_reads_only_backlights() {
        let devices = parse_device_list(
            "intel_backlight,backlight,400,40%\n\
             acpi_video0,backlight,20,2%\n\
             input2::kana,leds,0,0%,1\n",
        );
        assert_eq!(devices, ["intel_backlight", "acpi_video0"]);
    }

    #[test]
    fn device_list_ignores_leds_when_no_backlight_exists() {
        let devices = parse_device_list("input2::kana,leds,0,0%,1\n");
        assert!(devices.is_empty());
    }

    #[test]
    fn machine_output_reads_the_percent() {
        assert_eq!(
            parse_machine_output("intel_backlight,backlight,400,40%\n"),
            Some(40.0)
        );
        assert_eq!(
            parse_machine_output("intel_backlight,backlight,raw,400,1000,40%\n"),
            Some(40.0)
        );
    }

    #[test]
    fn the_maximum_field_is_not_mistaken_for_a_percentage() {
        assert_eq!(
            parse_machine_output("amdgpu_bl2,backlight,49603,87%,65535\n"),
            Some(87.0)
        );
    }

    #[test]
    fn machine_output_without_a_percent_is_rejected() {
        assert_eq!(parse_machine_output("garbage"), None);
        assert_eq!(
            parse_machine_output("amdgpu_bl2,backlight,49603,65535\n"),
            None
        );
    }
}
