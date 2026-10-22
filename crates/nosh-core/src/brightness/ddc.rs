//! DDC/CI external display brightness through VCP code `0x10`.
//!
//! `ddcutil` is the default implementation. The [`DdcBackend`] trait allows a
//! direct I2C implementation, but this crate ships only the process-based one
//! because `ddcutil` handles monitor quirks and multiple buses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::brightness::backend::{BrightnessBackend, BrightnessCapabilities};
use crate::brightness::model::{
    clamp_percent, linear_percent_from_raw, linear_raw_from_percent, BrightnessDeviceId,
    BrightnessDeviceInfo, BrightnessReading,
};
use crate::brightness::process::{command_exists, run_blocking};
use crate::error::BrightnessError;

/// Brightness VCP code.
pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const DEFAULT_DDC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdcDisplay {
    pub bus: u8,
    pub name: String,
    pub connector: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdcValue {
    pub current: u16,
    pub max: u16,
}

impl DdcValue {
    pub fn percent(self) -> f64 {
        linear_percent_from_raw(self.current as u32, self.max as u32)
    }
}

/// A synchronous DDC/CI implementation.
///
/// Calls run on a blocking task so a slow monitor never blocks the runtime.
pub trait DdcBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self) -> Result<Vec<DdcDisplay>, BrightnessError>;
    fn get(&self, display: &DdcDisplay) -> Result<DdcValue, BrightnessError>;
    fn set(&self, display: &DdcDisplay, value: u16) -> Result<(), BrightnessError>;
}

#[derive(Debug, Clone)]
pub struct DdcutilBackend {
    binary: String,
    timeout: Duration,
}

impl DdcutilBackend {
    pub fn new() -> Self {
        Self::with_config("ddcutil", DEFAULT_DDC_TIMEOUT)
    }

    pub fn with_config(binary: impl Into<String>, timeout: Duration) -> Self {
        Self {
            binary: binary.into(),
            timeout,
        }
    }

    pub fn available(&self) -> bool {
        command_exists(&self.binary)
    }

    fn run(&self, arguments: &[&str]) -> Result<String, BrightnessError> {
        run_blocking(&self.binary, arguments, self.timeout)
    }
}

impl Default for DdcutilBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DdcBackend for DdcutilBackend {
    fn name(&self) -> &'static str {
        "ddcutil"
    }

    fn detect(&self) -> Result<Vec<DdcDisplay>, BrightnessError> {
        let output = self.run(&["detect"])?;
        let displays = parse_detect(&output);
        if displays.is_empty() {
            return Err(BrightnessError::NotFound(
                "ddcutil reported no displays".to_string(),
            ));
        }
        Ok(displays)
    }

    fn get(&self, display: &DdcDisplay) -> Result<DdcValue, BrightnessError> {
        let bus = display.bus.to_string();
        let output = self.run(&["-b", &bus, "getvcp", "10"])?;
        parse_getvcp(&output)
            .ok_or_else(|| BrightnessError::Parse(format!("unexpected getvcp output {output:?}")))
    }

    fn set(&self, display: &DdcDisplay, value: u16) -> Result<(), BrightnessError> {
        let bus = display.bus.to_string();
        let value = value.to_string();
        self.run(&["-b", &bus, "setvcp", "10", &value])?;
        Ok(())
    }
}

/// Adapts any [`DdcBackend`] to the async device backend trait.
pub struct DdcBackendBridge {
    backend: Arc<dyn DdcBackend>,
    /// The last maximum value per bus, so writes do not need a second read.
    maxima: Mutex<HashMap<u8, u16>>,
}

impl DdcBackendBridge {
    pub fn new(backend: Arc<dyn DdcBackend>) -> Self {
        Self {
            backend,
            maxima: Mutex::new(HashMap::new()),
        }
    }

    fn display(device: &BrightnessDeviceId) -> Result<DdcDisplay, BrightnessError> {
        match device {
            BrightnessDeviceId::Ddc { bus, display } => Ok(DdcDisplay {
                bus: *bus,
                name: display.clone(),
                connector: Some(display.clone()),
            }),
            _ => Err(BrightnessError::Unsupported {
                capability: "ddc",
                device: device.stable_id(),
            }),
        }
    }

    fn cached_max(&self, bus: u8) -> Option<u16> {
        self.maxima.lock().ok()?.get(&bus).copied()
    }

    fn store_max(&self, bus: u8, max: u16) {
        if let Ok(mut maxima) = self.maxima.lock() {
            maxima.insert(bus, max);
        }
    }
}

#[async_trait]
impl BrightnessBackend for DdcBackendBridge {
    fn name(&self) -> &'static str {
        self.backend.name()
    }

    fn supports(&self, device: &BrightnessDeviceId) -> bool {
        matches!(device, BrightnessDeviceId::Ddc { .. })
    }

    async fn probe(&self) -> Result<BrightnessCapabilities, BrightnessError> {
        Ok(BrightnessCapabilities::CONTROL)
    }

    async fn discover(&self) -> Result<Vec<BrightnessDeviceInfo>, BrightnessError> {
        let backend = self.backend.clone();
        let displays = tokio::task::spawn_blocking(move || backend.detect())
            .await
            .map_err(|error| {
                BrightnessError::Unavailable(format!("DDC detection task failed: {error}"))
            })??;

        Ok(displays
            .into_iter()
            .map(|display| BrightnessDeviceInfo {
                id: BrightnessDeviceId::Ddc {
                    bus: display.bus,
                    display: display.name.clone(),
                },
                kind: crate::brightness::model::BrightnessKind::ExternalDdc,
                name: display.name,
                connector: display.connector,
                ddc_bus: Some(display.bus),
            })
            .collect())
    }

    async fn read(
        &self,
        device: &BrightnessDeviceId,
    ) -> Result<BrightnessReading, BrightnessError> {
        let display = Self::display(device)?;
        let bus = display.bus;
        let backend = self.backend.clone();
        let value = tokio::task::spawn_blocking(move || backend.get(&display))
            .await
            .map_err(|error| {
                BrightnessError::Unavailable(format!("DDC read task failed: {error}"))
            })??;
        self.store_max(bus, value.max);

        Ok(BrightnessReading {
            percent: value.percent(),
            raw: Some(value.current as u32),
            max_raw: Some(value.max as u32),
        })
    }

    async fn write(
        &self,
        device: &BrightnessDeviceId,
        percent: f64,
    ) -> Result<(), BrightnessError> {
        let display = Self::display(device)?;
        let bus = display.bus;
        let backend = self.backend.clone();

        let max = match self.cached_max(bus) {
            Some(max) => max,
            None => {
                let get = backend.clone();
                let display_for_get = display.clone();
                let value = tokio::task::spawn_blocking(move || get.get(&display_for_get))
                    .await
                    .map_err(|error| {
                        BrightnessError::Unavailable(format!("DDC read task failed: {error}"))
                    })??;
                self.store_max(bus, value.max);
                value.max
            }
        };

        let raw = linear_raw_from_percent(clamp_percent(percent), max as u32) as u16;
        tokio::task::spawn_blocking(move || backend.set(&display, raw))
            .await
            .map_err(|error| {
                BrightnessError::Unavailable(format!("DDC write task failed: {error}"))
            })??;
        Ok(())
    }
}

/// Parses `ddcutil detect`.
pub fn parse_detect(output: &str) -> Vec<DdcDisplay> {
    let mut displays = Vec::new();
    let mut bus: Option<u8> = None;
    let mut connector: Option<String> = None;
    let mut monitor: Option<String> = None;

    let flush = |displays: &mut Vec<DdcDisplay>,
                 bus: &mut Option<u8>,
                 connector: &mut Option<String>,
                 monitor: &mut Option<String>| {
        if let Some(bus) = bus.take() {
            let name = connector
                .clone()
                .or_else(|| monitor.clone())
                .unwrap_or_else(|| format!("display-{bus}"));
            displays.push(DdcDisplay {
                bus,
                name,
                connector: connector.take(),
            });
        }
        *monitor = None;
        *connector = None;
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Display ") {
            flush(&mut displays, &mut bus, &mut connector, &mut monitor);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("I2C bus:") {
            bus = parse_bus(rest);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("DRM connector:") {
            let value = normalize_connector(rest.trim());
            if !value.is_empty() {
                connector = Some(value);
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Monitor:") {
            let value = rest.trim();
            if !value.is_empty() {
                monitor = Some(value.to_string());
            }
        }
    }
    flush(&mut displays, &mut bus, &mut connector, &mut monitor);

    displays
}

fn parse_bus(value: &str) -> Option<u8> {
    let value = value.trim();
    let index = value.find("i2c-")? + 4;
    value[index..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn normalize_connector(value: &str) -> String {
    let Some(rest) = value.strip_prefix("card") else {
        return value.to_string();
    };
    match rest.split_once('-') {
        Some((_, connector)) => connector.to_string(),
        None => value.to_string(),
    }
}

/// Parses `ddcutil getvcp 10`, both brief and normal output.
pub fn parse_getvcp(output: &str) -> Option<DdcValue> {
    if let Some(value) = parse_brief_getvcp(output) {
        return Some(value);
    }

    let current = parse_after(output, "current value =")?;
    let max = parse_after(output, "max value =")?;
    Some(DdcValue { current, max })
}

fn parse_brief_getvcp(output: &str) -> Option<DdcValue> {
    for line in output.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 5 || tokens[0] != "VCP" {
            continue;
        }
        let Ok(current) = tokens[3].parse::<u16>() else {
            continue;
        };
        let Ok(max) = tokens[4].parse::<u16>() else {
            continue;
        };
        return Some(DdcValue { current, max });
    }
    None
}

fn parse_after(output: &str, marker: &str) -> Option<u16> {
    let rest = output.split(marker).nth(1)?;
    let value = rest
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETECT: &str = "\
Display 1
    I2C bus:  /dev/i2c-7
    DRM connector:           card0-DP-1
    Monitor:                 DEL:1234

Display 2
    I2C bus:  /dev/i2c-9
    Monitor:                 AOC:5678
";

    #[test]
    fn detect_reads_buses_connectors_and_monitors() {
        let displays = parse_detect(DETECT);
        assert_eq!(displays.len(), 2);
        assert_eq!(displays[0].bus, 7);
        assert_eq!(displays[0].name, "DP-1");
        assert_eq!(displays[0].connector.as_deref(), Some("DP-1"));
        assert_eq!(displays[1].bus, 9);
        assert_eq!(displays[1].name, "AOC:5678");
        assert_eq!(displays[1].connector, None);
    }

    #[test]
    fn brief_getvcp_is_parsed() {
        assert_eq!(
            parse_getvcp("VCP 10 C 80 100\n"),
            Some(DdcValue {
                current: 80,
                max: 100
            })
        );
    }

    #[test]
    fn normal_getvcp_is_parsed() {
        let output = "VCP code 0x10 (Brightness                    ): current value =    80, max value =   100\n";
        assert_eq!(
            parse_getvcp(output),
            Some(DdcValue {
                current: 80,
                max: 100
            })
        );
    }

    #[test]
    fn percent_uses_the_display_scale() {
        let value = DdcValue {
            current: 80,
            max: 100,
        };
        assert_eq!(value.percent(), 80.0);
    }
}
