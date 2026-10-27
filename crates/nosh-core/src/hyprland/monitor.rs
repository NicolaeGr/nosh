//! Hyprland monitor backend: `j/monitors` reads, the shared event stream, and
//! runtime `keyword monitor` writes.
//!
//! Writes use the command socket and require an `ok` reply. A rejected write
//! becomes a typed error that the service turns into `ApplyFailed`. This module
//! never writes a Hyprland config file and never reloads config.

use std::collections::HashSet;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::sync::broadcast::error::RecvError;

use crate::error::{BackendError, PathError};
use crate::hyprland::connection::{Hyprland, HyprlandState};
use crate::hyprland::dispatch::{escape_lua, ConfigProvider};
use crate::hyprland::events::HyprlandEvent;
use crate::hyprland::json::{build_monitor_layout, HyprMonitor};
use crate::monitors::backend::{MonitorBackend, MonitorBackendEvent};
use crate::monitors::model::{MonitorCapabilities, MonitorLayout, MonitorModel};
use crate::paths::HyprlandPaths;

pub struct HyprlandMonitorBackend {
    hypr: Arc<Hyprland>,
}

impl HyprlandMonitorBackend {
    /// Returns `None` when Hyprland is not the running compositor.
    pub fn detect() -> Result<Option<Self>, PathError> {
        Ok(Hyprland::detect()?.map(Self::with_connection))
    }

    pub fn new(paths: HyprlandPaths) -> Self {
        Self::with_connection(Hyprland::connect(paths))
    }

    pub fn with_connection(hypr: Arc<Hyprland>) -> Self {
        Self { hypr }
    }

    pub fn paths(&self) -> &HyprlandPaths {
        self.hypr.paths()
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
        self.hypr.refresh_provider().await;
        let _monitors: Vec<HyprMonitor> = self.hypr.query_json("j/monitors").await?;

        Ok(MonitorCapabilities::READ
            | MonitorCapabilities::WATCH
            | MonitorCapabilities::WRITE
            | MonitorCapabilities::MIRROR
            | MonitorCapabilities::ENABLE
            | MonitorCapabilities::SET_PRIMARY)
    }

    async fn read(&self) -> Result<MonitorLayout, BackendError> {
        let monitors: Vec<HyprMonitor> = self.hypr.query_json("j/monitors").await?;
        Ok(build_monitor_layout(&monitors))
    }

    async fn watch(&self, events: Sender<MonitorBackendEvent>) -> Result<(), BackendError> {
        let mut hypr_events = self.hypr.events();
        let mut state = self.hypr.state();
        let mut unknown = HashSet::new();

        loop {
            if *state.borrow_and_update() == HyprlandState::Lost {
                return Err(BackendError::Unavailable(
                    "Hyprland event socket lost".to_string(),
                ));
            }

            tokio::select! {
                changed = state.changed() => {
                    if changed.is_err() {
                        return Err(BackendError::Unavailable(
                            "Hyprland event socket lost".to_string(),
                        ));
                    }
                }
                received = hypr_events.recv() => {
                    match received {
                        Ok(HyprlandEvent::Connected) => {}
                        Ok(
                            HyprlandEvent::ConfigReloaded
                            | HyprlandEvent::MonitorAdded { .. }
                            | HyprlandEvent::MonitorRemoved { .. }
                            | HyprlandEvent::FocusedMonitor { .. },
                        ) => {
                            self.resync(&events).await?;
                        }
                        Ok(HyprlandEvent::Unknown { name, .. }) if unknown.insert(name.clone()) => {
                            tracing::debug!(event = %name, "ignoring unrelated Hyprland event");
                        }
                        Ok(_) => {}
                        Err(RecvError::Lagged(_)) => {
                            tracing::debug!("monitor event stream lagged; resyncing");
                            self.resync(&events).await?;
                        }
                        Err(RecvError::Closed) => {
                            return Err(BackendError::Unavailable(
                                "Hyprland event socket lost".to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn apply(&self, layout: &MonitorLayout) -> Result<(), BackendError> {
        tracing::debug!("applying a Hyprland monitor layout");

        let mut ordered: Vec<&MonitorModel> = layout.monitors.iter().collect();
        ordered.sort_by_key(|monitor| monitor.id.compositor_id);

        let provider = self.hypr.provider().await;
        let mut requests = Vec::new();
        for monitor in ordered.iter().filter(|monitor| !monitor.enabled) {
            requests.push(match provider {
                ConfigProvider::Hyprlang => {
                    format!("keyword {}", format_monitor_disable(&monitor.id.connector))
                }
                ConfigProvider::Lua => format_monitor_lua_disable(&monitor.id.connector),
            });
        }
        for monitor in ordered.iter().filter(|monitor| monitor.enabled) {
            requests.push(match provider {
                ConfigProvider::Hyprlang => format!("keyword {}", format_monitor_request(monitor)),
                ConfigProvider::Lua => format_monitor_lua(monitor),
            });
        }

        self.hypr.requests(&requests).await
    }
}

/// Formats one enabled monitor as a Hyprland runtime keyword argument.
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
            "monitor \"{},{},{position},{scale},mirror,{source}\"",
            monitor.connector(),
            mode,
        ),
        None => format!(
            "monitor \"{},{},{position},{scale}\"",
            monitor.connector(),
            mode,
        ),
    }
}

/// Formats the keyword argument that disables a monitor.
pub fn format_monitor_disable(connector: &str) -> String {
    format!("monitor \"{connector},disable\"")
}

/// Formats an enabled monitor as an `hl.monitor(...)` Lua call.
pub fn format_monitor_lua(monitor: &MonitorModel) -> String {
    let mode = monitor_mode(monitor);
    let position = format!("{}x{}", monitor.x, monitor.y);
    let scale = if monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };

    let mut fields = vec![
        format!("output = \"{}\"", escape_lua(monitor.connector())),
        format!("mode = \"{}\"", escape_lua(&mode)),
        format!("position = \"{position}\""),
        format!("scale = {scale}"),
    ];

    if let Some(source) = monitor
        .mirror_of
        .as_deref()
        .filter(|source| *source != monitor.connector())
    {
        fields.push(format!("mirror = \"{}\"", escape_lua(source)));
    }

    format!("eval hl.monitor({{ {} }})", fields.join(", "))
}

/// Formats the Lua call that disables a monitor.
pub fn format_monitor_lua_disable(connector: &str) -> String {
    format!(
        "eval hl.monitor({{ output = \"{}\", disabled = true }})",
        escape_lua(connector)
    )
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
            "monitor \"DP-1,2560x1440@144.00,1920x0,1\""
        );
    }

    #[test]
    fn a_mirror_adds_the_source() {
        let mut monitor = monitor("DP-1");
        monitor.mirror_of = Some("DP-2".to_string());

        assert_eq!(
            format_monitor_request(&monitor),
            "monitor \"DP-1,2560x1440@144.00,1920x0,1,mirror,DP-2\""
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
            "monitor \"HDMI-A-1,disable\""
        );
    }

    #[test]
    fn an_enabled_monitor_formats_its_lua_fields() {
        assert_eq!(
            format_monitor_lua(&monitor("DP-1")),
            "eval hl.monitor({ output = \"DP-1\", mode = \"2560x1440@144.00\", position = \"1920x0\", scale = 1 })"
        );
    }

    #[test]
    fn a_mirror_adds_the_lua_source() {
        let mut monitor = monitor("DP-1");
        monitor.mirror_of = Some("DP-2".to_string());

        assert_eq!(
            format_monitor_lua(&monitor),
            "eval hl.monitor({ output = \"DP-1\", mode = \"2560x1440@144.00\", position = \"1920x0\", scale = 1, mirror = \"DP-2\" })"
        );
    }

    #[test]
    fn a_disabled_monitor_formats_the_lua_disable_call() {
        assert_eq!(
            format_monitor_lua_disable("HDMI-A-1"),
            "eval hl.monitor({ output = \"HDMI-A-1\", disabled = true })"
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
