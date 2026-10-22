//! Hyprland monitor backend: `j/monitors` reads, the event socket, and runtime
//! `keyword monitor` writes.
//!
//! Writes use the command socket and require an `ok` reply. A rejected write
//! becomes a typed error that the service turns into `ApplyFailed`. This module
//! never writes a Hyprland config file and never reloads config.

use std::collections::HashSet;
use std::time::Duration;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::RwLock;
use tokio::time::timeout;

use crate::error::{BackendError, PathError};
use crate::hyprland::client::HyprlandClient;
use crate::hyprland::dispatch::{detect_provider, ConfigProvider};
use crate::hyprland::events::{parse_event, HyprlandEvent};
use crate::hyprland::json::{build_monitor_layout, HyprMonitor, HyprStatus, HyprVersion};
use crate::monitors::backend::{MonitorBackend, MonitorBackendEvent};
use crate::monitors::model::{MonitorCapabilities, MonitorLayout, MonitorModel};
use crate::paths::HyprlandPaths;

const EVENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct HyprlandMonitorBackend {
    paths: HyprlandPaths,
    client: HyprlandClient,
    provider: RwLock<Option<ConfigProvider>>,
}

impl HyprlandMonitorBackend {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn detect() -> Result<Option<Self>, PathError> {
        Ok(HyprlandPaths::from_env()?.map(Self::new))
    }

    pub fn new(paths: HyprlandPaths) -> Self {
        let client = HyprlandClient::new(paths.command_socket.clone());
        Self {
            paths,
            client,
            provider: RwLock::new(None),
        }
    }

    pub fn paths(&self) -> &HyprlandPaths {
        &self.paths
    }

    async fn provider(&self) -> ConfigProvider {
        if let Some(provider) = *self.provider.read().await {
            return provider;
        }

        let provider = self.query_provider().await;
        *self.provider.write().await = Some(provider);
        provider
    }

    async fn query_provider(&self) -> ConfigProvider {
        let status: Option<HyprStatus> = match self.client.query_json("j/status").await {
            Ok(status) => Some(status),
            Err(error) => {
                tracing::debug!(%error, "Hyprland status query failed");
                None
            }
        };
        let version: Option<HyprVersion> = match self.client.query_json("j/version").await {
            Ok(version) => Some(version),
            Err(error) => {
                tracing::debug!(%error, "Hyprland version query failed");
                None
            }
        };

        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            version.as_ref().map(|version| version.tag.as_str()),
        );
        tracing::debug!(?provider, "detected Hyprland config provider");
        provider
    }

    async fn refresh_provider(&self) {
        let provider = self.query_provider().await;
        *self.provider.write().await = Some(provider);
    }

    async fn send_keyword(&self, request: &str) -> Result<(), BackendError> {
        let response = self.client.query(request).await?;
        if response.trim().eq_ignore_ascii_case("ok") {
            Ok(())
        } else {
            Err(BackendError::Request(format!(
                "Hyprland rejected `{request}`: {}",
                response.trim()
            )))
        }
    }

    async fn resync(&self, events: &Sender<MonitorBackendEvent>) -> Result<(), BackendError> {
        let layout = self.read().await?;
        if events
            .send(MonitorBackendEvent::Layout(layout))
            .await
            .is_err()
        {
            return Err(BackendError::Unavailable(
                "monitor event receiver closed".to_string(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl MonitorBackend for HyprlandMonitorBackend {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    async fn probe(&self) -> Result<MonitorCapabilities, BackendError> {
        let version: HyprVersion = self.client.query_json("j/version").await?;
        let _monitors: Vec<HyprMonitor> = self.client.query_json("j/monitors").await?;
        let status: Option<HyprStatus> = match self.client.query_json("j/status").await {
            Ok(status) => Some(status),
            Err(error) => {
                tracing::debug!(%error, "Hyprland status query failed during probe");
                None
            }
        };

        let provider = detect_provider(
            status
                .as_ref()
                .and_then(|status| status.config_provider.as_deref()),
            Some(version.tag.as_str()),
        );
        *self.provider.write().await = Some(provider);

        Ok(MonitorCapabilities::READ
            | MonitorCapabilities::WATCH
            | MonitorCapabilities::WRITE
            | MonitorCapabilities::MIRROR
            | MonitorCapabilities::ENABLE
            | MonitorCapabilities::SET_PRIMARY)
    }

    async fn read(&self) -> Result<MonitorLayout, BackendError> {
        let monitors: Vec<HyprMonitor> = self.client.query_json("j/monitors").await?;
        Ok(build_monitor_layout(&monitors))
    }

    async fn watch(&self, events: Sender<MonitorBackendEvent>) -> Result<(), BackendError> {
        let stream = timeout(
            EVENT_CONNECT_TIMEOUT,
            UnixStream::connect(&self.paths.event_socket),
        )
        .await
        .map_err(|_| {
            BackendError::Unavailable(format!(
                "timed out connecting to {}",
                self.paths.event_socket.display()
            ))
        })?
        .map_err(|error| {
            BackendError::Unavailable(format!(
                "cannot connect to {}: {error}",
                self.paths.event_socket.display()
            ))
        })?;

        let mut lines = BufReader::new(stream).lines();
        let mut unknown = HashSet::new();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => {
                    return Err(BackendError::Unavailable(
                        "Hyprland event socket closed".to_string(),
                    ));
                }
                Err(error) => {
                    return Err(BackendError::Request(format!(
                        "cannot read the Hyprland event socket: {error}"
                    )));
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            let event = match parse_event(&line) {
                Ok(event) => event,
                Err(error) => {
                    tracing::debug!(%error, line, "ignoring malformed Hyprland event");
                    continue;
                }
            };

            match event {
                HyprlandEvent::ConfigReloaded => {
                    self.refresh_provider().await;
                    self.resync(&events).await?;
                }
                HyprlandEvent::MonitorAdded { .. }
                | HyprlandEvent::MonitorRemoved { .. }
                | HyprlandEvent::FocusedMonitor { .. } => {
                    self.resync(&events).await?;
                }
                HyprlandEvent::Unknown { name, .. } if unknown.insert(name.clone()) => {
                    tracing::debug!(event = %name, "ignoring unrelated Hyprland event");
                }
                _ => {}
            }
        }
    }

    async fn apply(&self, layout: &MonitorLayout) -> Result<(), BackendError> {
        let provider = self.provider().await;
        tracing::debug!(?provider, "applying a Hyprland monitor layout");

        let mut ordered: Vec<&MonitorModel> = layout.monitors.iter().collect();
        ordered.sort_by_key(|monitor| monitor.id.compositor_id);

        for monitor in ordered.iter().filter(|monitor| !monitor.enabled) {
            self.send_keyword(&format_monitor_disable(&monitor.id.connector))
                .await?;
        }
        for monitor in ordered.iter().filter(|monitor| monitor.enabled) {
            self.send_keyword(&format_monitor_request(monitor)).await?;
        }

        Ok(())
    }
}

/// Formats one enabled monitor as a Hyprland runtime keyword request.
pub fn format_monitor_request(monitor: &MonitorModel) -> String {
    let mode = monitor_mode(monitor);
    let position = format!("{}x{}", monitor.x, monitor.y);
    let scale = if monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };

    match monitor
        .mirror_of
        .as_deref()
        .filter(|source| *source != monitor.connector())
    {
        Some(source) => format!(
            "keyword monitor \"{},{},{position},{scale},mirror,{source}\"",
            monitor.connector(),
            mode,
        ),
        None => format!(
            "keyword monitor \"{},{},{position},{scale}\"",
            monitor.connector(),
            mode,
        ),
    }
}

/// Formats the request that disables a monitor.
pub fn format_monitor_disable(connector: &str) -> String {
    format!("keyword monitor \"{connector},disable\"")
}

fn monitor_mode(monitor: &MonitorModel) -> String {
    if monitor.width <= 0 || monitor.height <= 0 {
        return "preferred".to_string();
    }

    if monitor.refresh_rate > 0.0 {
        format!(
            "{}x{}@{:.2}",
            monitor.width, monitor.height, monitor.refresh_rate
        )
    } else {
        format!("{}x{}", monitor.width, monitor.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::model::{MonitorId, Transform};

    fn monitor(connector: &str) -> MonitorModel {
        MonitorModel {
            id: MonitorId {
                compositor_id: 1,
                connector: connector.to_string(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
            },
            enabled: true,
            focused: false,
            primary: false,
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
            scale: 1.0,
            transform: Transform::Normal,
            refresh_rate: 144.0,
            mirror_of: None,
        }
    }

    #[test]
    fn an_enabled_monitor_formats_its_resolution_position_and_scale() {
        assert_eq!(
            format_monitor_request(&monitor("DP-1")),
            "keyword monitor \"DP-1,2560x1440@144.00,1920x0,1\""
        );
    }

    #[test]
    fn a_mirror_adds_the_source() {
        let mut monitor = monitor("DP-1");
        monitor.mirror_of = Some("DP-2".to_string());

        assert_eq!(
            format_monitor_request(&monitor),
            "keyword monitor \"DP-1,2560x1440@144.00,1920x0,1,mirror,DP-2\""
        );
    }

    #[test]
    fn a_self_mirror_is_ignored() {
        let mut monitor = monitor("DP-1");
        monitor.mirror_of = Some("DP-1".to_string());

        assert!(!format_monitor_request(&monitor).contains("mirror"));
    }

    #[test]
    fn a_disabled_monitor_formats_the_disable_keyword() {
        assert_eq!(
            format_monitor_disable("HDMI-A-1"),
            "keyword monitor \"HDMI-A-1,disable\""
        );
    }

    #[test]
    fn missing_geometry_falls_back_to_preferred() {
        let mut monitor = monitor("DP-1");
        monitor.width = 0;
        monitor.height = 0;

        assert!(format_monitor_request(&monitor).contains(",preferred,"));
    }
}
