//! Hyprland JSON shapes and conversion to core models.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use crate::monitors::{MonitorId, MonitorLayout, MonitorModel, Transform};
use crate::workspaces::model::{
    normalize_address, WorkspaceCapabilities, WorkspaceModel, WorkspaceMonitor,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HyprMonitor {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub width: i32,
    pub height: i32,
    pub refresh_rate: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: i32,
    pub focused: bool,
    pub dpms_status: bool,
    pub disabled: bool,
    pub mirror_of: Option<String>,
    pub active_workspace: Option<HyprWorkspace>,
    pub special_workspace: Option<HyprWorkspace>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HyprWorkspace {
    pub id: i64,
    pub name: String,
    pub monitor_id: i32,
    pub monitor: Option<String>,
    pub has_fullscreen: bool,
    pub last_window: String,
    pub windows: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HyprClient {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    pub workspace: Option<HyprWorkspace>,
    pub floating: bool,
    pub monitor: i32,
    pub class: String,
    pub title: String,
    pub initial_class: String,
    pub initial_title: String,
    pub pid: u32,
    pub xwayland: bool,
    pub pinned: bool,
    pub fullscreen: i32,
    pub fullscreen_client: i32,
    pub inhibiting_idle: bool,
    pub swallowing: String,
    pub focus_history_id: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HyprStatus {
    pub config_provider: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HyprVersion {
    pub tag: String,
    pub commit: String,
    pub branch: String,
    pub flags: Vec<String>,
}

/// Builds the workspace models, monitor models, client map, and focus key from
/// a Hyprland read.
pub fn build_workspace_state(
    monitors: &[HyprMonitor],
    workspaces: &[HyprWorkspace],
    active: Option<&HyprWorkspace>,
    clients: &[HyprClient],
) -> (
    Vec<WorkspaceModel>,
    Vec<WorkspaceMonitor>,
    BTreeMap<String, String>,
    Option<String>,
) {
    let monitor_names: HashMap<i32, String> = monitors
        .iter()
        .map(|monitor| (monitor.id, monitor.name.clone()))
        .collect();

    let monitor_models = monitors
        .iter()
        .filter(|monitor| !monitor.disabled)
        .map(|monitor| WorkspaceMonitor {
            id: monitor.id,
            connector: monitor.name.clone(),
            focused: monitor.focused,
        })
        .collect();

    let workspace_models = workspaces
        .iter()
        .filter(|workspace| is_normal_workspace(workspace))
        .map(|workspace| WorkspaceModel {
            id: workspace.id.to_string(),
            name: workspace.name.clone(),
            monitor: workspace
                .monitor
                .clone()
                .or_else(|| monitor_names.get(&workspace.monitor_id).cloned()),
            special: false,
            has_fullscreen: workspace.has_fullscreen,
            focused: false,
            urgent: false,
            capabilities: WorkspaceCapabilities::READ
                | WorkspaceCapabilities::WATCH
                | WorkspaceCapabilities::ACTIVATE
                | WorkspaceCapabilities::MOVE
                | WorkspaceCapabilities::URGENT
                | WorkspaceCapabilities::SPECIAL,
        })
        .collect();

    let clients = clients
        .iter()
        .filter_map(|client| {
            client
                .workspace
                .as_ref()
                .map(|workspace| (normalize_address(&client.address), workspace.id.to_string()))
        })
        .collect();

    let focused = active
        .filter(|workspace| is_normal_workspace(workspace))
        .map(|workspace| workspace.id.to_string());

    (workspace_models, monitor_models, clients, focused)
}

/// A workspace is normal when its id is positive and its name has no `special:`
/// prefix.
pub fn is_normal_workspace(workspace: &HyprWorkspace) -> bool {
    workspace.id > 0 && !workspace.name.starts_with("special:")
}

/// Maps a raw `j/monitors` response to the core layout.
///
/// Disabled monitors stay in the list with `enabled = false`, and callers filter
/// them out. The layout is sorted by compositor id so numeric targets keep the
/// same meaning.
pub fn build_monitor_layout(monitors: &[HyprMonitor]) -> MonitorLayout {
    let mut models: Vec<MonitorModel> = monitors.iter().map(monitor_model).collect();
    models.sort_by_key(|monitor| monitor.id.compositor_id);

    let mut layout = MonitorLayout {
        monitors: models,
        primary: None,
    };
    let _ = layout.apply_primary(None);
    layout
}

fn monitor_model(monitor: &HyprMonitor) -> MonitorModel {
    MonitorModel {
        id: MonitorId {
            compositor_id: monitor.id,
            connector: monitor.name.clone(),
            description: monitor.description.clone(),
            make: monitor.make.clone(),
            model: monitor.model.clone(),
            serial: monitor.serial.clone(),
        },
        enabled: !monitor.disabled,
        focused: monitor.focused,
        primary: false,
        x: monitor.x,
        y: monitor.y,
        width: monitor.width,
        height: monitor.height,
        scale: monitor.scale,
        transform: Transform::from_value(monitor.transform).unwrap_or_default(),
        refresh_rate: monitor.refresh_rate,
        mirror_of: mirror_source(monitor),
    }
}

fn mirror_source(monitor: &HyprMonitor) -> Option<String> {
    monitor
        .mirror_of
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty() && !source.eq_ignore_ascii_case("none"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: i32, name: &str, focused: bool, disabled: bool) -> HyprMonitor {
        HyprMonitor {
            id,
            name: name.to_string(),
            focused,
            disabled,
            ..HyprMonitor::default()
        }
    }

    fn workspace(id: i64, name: &str, monitor_id: i32) -> HyprWorkspace {
        HyprWorkspace {
            id,
            name: name.to_string(),
            monitor_id,
            ..HyprWorkspace::default()
        }
    }

    #[test]
    fn json_fields_use_camel_case() {
        let monitor: HyprMonitor = serde_json::from_str(
            r#"{"id":1,"name":"DP-1","refreshRate":144.0,"dpmsStatus":true,"activeWorkspace":{"id":2,"name":"2"}}"#,
        )
        .unwrap();

        assert_eq!(monitor.name, "DP-1");
        assert_eq!(monitor.refresh_rate, 144.0);
        assert!(monitor.dpms_status);
        assert_eq!(monitor.active_workspace.unwrap().id, 2);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let monitor: HyprMonitor = serde_json::from_str(r#"{"id":1,"name":"DP-1"}"#).unwrap();
        assert_eq!(monitor.width, 0);
        assert_eq!(monitor.scale, 0.0);
    }

    #[test]
    fn special_and_non_positive_workspaces_are_filtered() {
        let monitors = vec![monitor(0, "DP-1", true, false)];
        let workspaces = vec![
            workspace(1, "1", 0),
            workspace(0, "0", 0),
            workspace(-1, "special:magic", 0),
            workspace(-99, "special:scratch", 0),
            workspace(2, "special:name", 0),
        ];

        let (models, _, _, _) = build_workspace_state(&monitors, &workspaces, None, &[]);

        let ids: Vec<&str> = models
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect();
        assert_eq!(ids, ["1"]);
        assert!(models.iter().all(|workspace| !workspace.special));
    }

    #[test]
    fn workspaces_keep_their_monitor_connector() {
        let monitors = vec![
            monitor(0, "DP-1", true, false),
            monitor(1, "HDMI-A-1", false, false),
        ];
        let workspaces = vec![workspace(1, "1", 0), workspace(2, "2", 1)];

        let (models, _, _, _) = build_workspace_state(&monitors, &workspaces, None, &[]);

        assert_eq!(models[0].monitor.as_deref(), Some("DP-1"));
        assert_eq!(models[1].monitor.as_deref(), Some("HDMI-A-1"));
    }

    #[test]
    fn disabled_monitors_are_not_exposed() {
        let monitors = vec![
            monitor(0, "DP-1", true, false),
            monitor(1, "DP-2", false, true),
        ];

        let (_, models, _, _) = build_workspace_state(&monitors, &[], None, &[]);

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].connector, "DP-1");
    }

    #[test]
    fn the_active_workspace_sets_focus() {
        let monitors = vec![monitor(0, "DP-1", true, false)];
        let workspaces = vec![workspace(1, "1", 0), workspace(2, "2", 0)];
        let active = workspace(2, "2", 0);

        let (_, _, _, focused) = build_workspace_state(&monitors, &workspaces, Some(&active), &[]);

        assert_eq!(focused.as_deref(), Some("2"));
    }

    #[test]
    fn special_active_workspaces_do_not_set_focus() {
        let monitors = vec![monitor(0, "DP-1", true, false)];
        let active = workspace(-1, "special:magic", 0);

        let (_, _, _, focused) = build_workspace_state(&monitors, &[], Some(&active), &[]);

        assert_eq!(focused, None);
    }

    #[test]
    fn clients_map_to_workspace_keys() {
        let clients = vec![HyprClient {
            address: "0xABC".to_string(),
            workspace: Some(workspace(2, "2", 0)),
            ..HyprClient::default()
        }];

        let (_, _, map, _) = build_workspace_state(&[], &[], None, &clients);

        assert_eq!(map.get("abc").map(String::as_str), Some("2"));
    }

    #[test]
    fn monitor_json_maps_every_model_field() {
        let json = r#"[{
            "id": 3,
            "name": "DP-2",
            "description": "Dell Inc. DELL U2720Q",
            "make": "Dell Inc.",
            "model": "DELL U2720Q",
            "serial": "ABC123",
            "width": 3840,
            "height": 2160,
            "refreshRate": 59.997,
            "x": 2560,
            "y": -100,
            "scale": 1.5,
            "transform": 2,
            "focused": true,
            "disabled": false,
            "mirrorOf": "DP-1"
        }]"#;
        let monitors: Vec<HyprMonitor> = serde_json::from_str(json).unwrap();

        let layout = build_monitor_layout(&monitors);
        let monitor = &layout.monitors[0];

        assert_eq!(monitor.id.compositor_id, 3);
        assert_eq!(monitor.connector(), "DP-2");
        assert_eq!(monitor.id.description, "Dell Inc. DELL U2720Q");
        assert_eq!(monitor.id.make, "Dell Inc.");
        assert_eq!(monitor.id.model, "DELL U2720Q");
        assert_eq!(monitor.id.serial, "ABC123");
        assert!(monitor.enabled);
        assert!(monitor.focused);
        assert!(monitor.primary);
        assert_eq!((monitor.x, monitor.y), (2560, -100));
        assert_eq!((monitor.width, monitor.height), (3840, 2160));
        assert_eq!(monitor.scale, 1.5);
        assert_eq!(monitor.transform, Transform::Rotate180);
        assert_eq!(monitor.refresh_rate, 59.997);
        assert_eq!(monitor.mirror_of.as_deref(), Some("DP-1"));
    }

    #[test]
    fn disabled_monitors_remain_visible_but_are_not_enabled() {
        let monitors = vec![
            monitor(0, "DP-1", true, false),
            monitor(1, "HDMI-A-1", false, true),
        ];

        let layout = build_monitor_layout(&monitors);

        assert_eq!(layout.monitors.len(), 2);
        assert_eq!(layout.enabled().count(), 1);
        assert!(!layout.monitor("HDMI-A-1").unwrap().enabled);
    }

    #[test]
    fn the_hyprland_mirror_none_sentinel_is_not_a_mirror() {
        let json = r#"[{"id":0,"name":"DP-1","mirrorOf":"none"}]"#;
        let monitors: Vec<HyprMonitor> = serde_json::from_str(json).unwrap();

        assert_eq!(build_monitor_layout(&monitors).monitors[0].mirror_of, None);
    }

    #[test]
    fn a_missing_mirror_field_is_not_a_mirror() {
        let json = r#"[{"id":0,"name":"DP-1"}]"#;
        let monitors: Vec<HyprMonitor> = serde_json::from_str(json).unwrap();

        assert_eq!(build_monitor_layout(&monitors).monitors[0].mirror_of, None);
    }

    #[test]
    fn the_mapped_layout_uses_focus_then_the_first_enabled_monitor() {
        let focused = vec![
            monitor(0, "DP-1", false, false),
            monitor(1, "HDMI-A-1", true, false),
        ];
        assert_eq!(
            build_monitor_layout(&focused).primary.as_deref(),
            Some("HDMI-A-1")
        );

        let unfocused = vec![
            monitor(2, "DP-2", false, false),
            monitor(1, "HDMI-A-1", false, false),
        ];
        assert_eq!(
            build_monitor_layout(&unfocused).primary.as_deref(),
            Some("HDMI-A-1")
        );
    }
}
