//! Sysfs backlight discovery with logind writes.
//!
//! Reads come from `/sys/class/backlight/<name>/brightness`. Writes go through
//! `org.freedesktop.login1.Session.SetBrightness` so the shell does not need
//! permission to write sysfs directly. `busctl` is used when it is installed,
//! with `dbus-send` as a second way to reach the same method.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::brightness::backend::{BrightnessBackend, BrightnessCapabilities};
use crate::brightness::model::{
    percent_from_raw, raw_from_percent, BrightnessDeviceId, BrightnessDeviceInfo, BrightnessReading,
};
use crate::brightness::process::{command_exists, run_async};
use crate::error::BrightnessError;

/// The device root. Tests point this at a temporary directory.
pub const SYSFS_ROOT: &str = "/sys/class/backlight";

#[derive(Debug, Clone)]
pub struct SysfsLogindConfig {
    pub root: PathBuf,
    pub busctl_binary: String,
    pub dbus_send_binary: String,
    pub timeout: Duration,
}

impl Default for SysfsLogindConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from(SYSFS_ROOT),
            busctl_binary: "busctl".to_string(),
            dbus_send_binary: "dbus-send".to_string(),
            timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SysfsLogindBackend {
    config: SysfsLogindConfig,
}

impl SysfsLogindBackend {
    pub fn new() -> Self {
        Self::with_config(SysfsLogindConfig::default())
    }

    pub fn with_config(config: SysfsLogindConfig) -> Self {
        Self { config }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.config.root.join(name)
    }

    async fn read_number(&self, name: &str, file: &str) -> Result<u32, BrightnessError> {
        let path = self.path(name).join(file);
        let text = tokio::fs::read_to_string(&path).await.map_err(|error| {
            BrightnessError::Unavailable(format!("cannot read {}: {error}", path.display()))
        })?;
        text.trim()
            .parse::<u32>()
            .map_err(|error| BrightnessError::Parse(format!("{}: {error}", path.display())))
    }

    async fn write_logind(&self, device: &str, raw: u32) -> Result<(), BrightnessError> {
        let raw = raw.to_string();
        let busctl_error = if command_exists(&self.config.busctl_binary) {
            let arguments = [
                "call",
                "org.freedesktop.login1",
                "/org/freedesktop/login1/session/auto",
                "org.freedesktop.login1.Session",
                "SetBrightness",
                "ssu",
                "backlight",
                device,
                raw.as_str(),
            ];
            match run_async(&self.config.busctl_binary, &arguments, self.config.timeout).await {
                Ok(_) => return Ok(()),
                Err(error) => Some(error),
            }
        } else {
            None
        };

        if command_exists(&self.config.dbus_send_binary) {
            let value = format!("uint32:{raw}");
            let arguments = [
                "--system",
                "--print-reply",
                "--dest=org.freedesktop.login1",
                "/org/freedesktop/login1/session/auto",
                "org.freedesktop.login1.Session.SetBrightness",
                "string:backlight",
                &format!("string:{device}"),
                value.as_str(),
            ];
            match run_async(
                &self.config.dbus_send_binary,
                &arguments,
                self.config.timeout,
            )
            .await
            {
                Ok(_) => return Ok(()),
                Err(error) => {
                    return Err(busctl_error.unwrap_or(error));
                }
            }
        }

        Err(busctl_error.unwrap_or_else(|| {
            BrightnessError::Unavailable(
                "neither busctl nor dbus-send is installed for logind writes".to_string(),
            )
        }))
    }
}

impl Default for SysfsLogindBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BrightnessBackend for SysfsLogindBackend {
    fn name(&self) -> &'static str {
        "sysfs-logind"
    }

    fn supports(&self, device: &BrightnessDeviceId) -> bool {
        match device {
            BrightnessDeviceId::Backlight(name) => self.path(name).is_dir(),
            _ => false,
        }
    }

    async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError> {
        if !self.config.root.is_dir() {
            return Err(BrightnessError::Unavailable(format!(
                "{} does not exist",
                self.config.root.display()
            )));
        }
        match self.discover().await {
            Ok(devices) if !devices.is_empty() => Ok(BrightnessCapabilities::CONTROL),
            Ok(_) => Err(BrightnessError::Unavailable(
                "no backlight devices were found".to_string(),
            )),
            Err(error) => Err(error),
        }
    }

    async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError> {
        let mut entries = tokio::fs::read_dir(&self.config.root)
            .await
            .map_err(|error| {
                BrightnessError::Unavailable(format!(
                    "cannot read {}: {error}",
                    self.config.root.display()
                ))
            })?;

        let mut found = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            BrightnessError::Unavailable(format!(
                "cannot read {}: {error}",
                self.config.root.display()
            ))
        })? {
            let metadata = entry.metadata().await.map_err(|error| {
                BrightnessError::Unavailable(format!(
                    "cannot read {}: {error}",
                    entry.path().display()
                ))
            })?;
            if !metadata.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if self.read_number(&name, "max_brightness").await.is_err() {
                continue;
            }
            found.push(name);
        }

        let mut scored: Vec<(i32, String)> = Vec::with_capacity(found.len());
        for name in found {
            let kind = tokio::fs::read_to_string(self.path(&name).join("type"))
                .await
                .unwrap_or_default();
            scored.push((backlight_score(&name, kind.trim()), name));
        }
        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

        Ok(scored
            .into_iter()
            .map(|(_, name)| BrightnessDeviceInfo::internal(name))
            .collect())
    }

    async fn read(
        &self,
        device: &BrightnessDeviceId,
    ) -> Result<BrightnessReading, BrightnessError> {
        let BrightnessDeviceId::Backlight(name) = device else {
            return Err(BrightnessError::Unsupported {
                capability: "read",
                device: device.stable_id(),
            });
        };
        let raw = self.read_number(name, "brightness").await?;
        let max = self.read_number(name, "max_brightness").await?;
        Ok(BrightnessReading {
            percent: percent_from_raw(raw, max),
            raw: Some(raw),
            max_raw: Some(max),
        })
    }

    async fn write(
        &self,
        device: &BrightnessDeviceId,
        percent: f64,
    ) -> Result<(), BrightnessError> {
        let BrightnessDeviceId::Backlight(name) = device else {
            return Err(BrightnessError::Unsupported {
                capability: "write",
                device: device.stable_id(),
            });
        };
        let max = self.read_number(name, "max_brightness").await?;
        let raw = raw_from_percent(percent, max);
        self.write_logind(name, raw).await
    }
}

/// Scores internal backlights. Raw devices beat platform and firmware devices.
/// Common GPU driver names beat ACPI video devices.
pub fn backlight_score(name: &str, kind: &str) -> i32 {
    let mut score = match kind {
        "raw" => 30,
        "platform" => 20,
        "firmware" => 10,
        _ => 0,
    };

    let lower = name.to_ascii_lowercase();
    if ["intel", "amdgpu", "radeon", "nvidia"]
        .iter()
        .any(|driver| lower.contains(driver))
    {
        score += 20;
    }
    if ["acpi_video", "video", "firmware"]
        .iter()
        .any(|generic| lower.contains(generic))
    {
        score -= 20;
    }
    score
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn backlight(root: &Path, name: &str, kind: &str, brightness: u32, max: u32) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("type"), kind).unwrap();
        fs::write(directory.join("brightness"), brightness.to_string()).unwrap();
        fs::write(directory.join("max_brightness"), max.to_string()).unwrap();
    }

    #[test]
    fn raw_gpu_backlights_beat_acpi_video() {
        assert!(
            backlight_score("intel_backlight", "raw") > backlight_score("acpi_video0", "firmware")
        );
        assert!(backlight_score("amdgpu_bl0", "raw") > backlight_score("acpi_video0", "platform"));
        assert!(backlight_score("acpi_video0", "firmware") < backlight_score("acpi_video0", "raw"));
    }

    #[tokio::test]
    async fn discovery_sorts_the_preferred_backlight_first() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        backlight(root, "acpi_video0", "firmware", 20, 100);
        backlight(root, "intel_backlight", "raw", 500, 1000);

        let backend = SysfsLogindBackend::with_config(SysfsLogindConfig {
            root: root.to_path_buf(),
            ..SysfsLogindConfig::default()
        });

        let devices = backend.discover().await.unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(
            devices[0].id,
            BrightnessDeviceId::Backlight("intel_backlight".into())
        );
    }

    #[tokio::test]
    async fn reading_applies_the_gamma_curve() {
        let directory = tempdir().unwrap();
        let root = directory.path();
        backlight(root, "intel_backlight", "raw", 250, 1000);

        let backend = SysfsLogindBackend::with_config(SysfsLogindConfig {
            root: root.to_path_buf(),
            ..SysfsLogindConfig::default()
        });

        let reading = backend
            .read(&BrightnessDeviceId::Backlight("intel_backlight".into()))
            .await
            .unwrap();
        assert_eq!(reading.percent, 50.0);
        assert_eq!(reading.raw, Some(250));
        assert_eq!(reading.max_raw, Some(1000));
    }

    #[tokio::test]
    async fn a_missing_root_is_unavailable() {
        let backend = SysfsLogindBackend::with_config(SysfsLogindConfig {
            root: PathBuf::from("/nosh/does/not/exist"),
            ..SysfsLogindConfig::default()
        });
        assert!(backend.probe().await.is_err());
    }
}
