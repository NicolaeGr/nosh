//! Parser for the Hyprland event socket.
//!
//! Each line is `event_name>>payload` and some events have no payload. The
//! parser validates payloads before indexing, so a malformed line cannot panic
//! the service.

use crate::error::EventParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HyprlandEvent {
    Workspace {
        id: i64,
        name: String,
    },
    FocusedMonitor {
        monitor: String,
        workspace: String,
    },
    CreateWorkspace {
        id: i64,
        name: String,
    },
    DestroyWorkspace {
        id: i64,
        name: String,
    },
    MoveWorkspace {
        id: i64,
        name: String,
        monitor: String,
    },
    RenameWorkspace {
        id: i64,
        name: String,
    },
    ActiveSpecial {
        name: String,
        monitor: String,
    },
    OpenWindow {
        address: String,
        workspace: String,
        class: String,
        title: String,
    },
    CloseWindow {
        address: String,
    },
    MoveWindow {
        address: String,
        workspace: String,
        monitor: String,
    },
    Urgent {
        address: String,
    },
    ActiveWindow {
        address: String,
    },
    ChangeFloatingMode {
        address: String,
        floating: bool,
    },
    Fullscreen {
        fullscreen: bool,
    },
    WindowTitle {
        address: String,
        title: String,
    },
    ConfigReloaded,
    /// Emitted by the connection reader after the event socket connects.
    Connected,
    MonitorAdded {
        id: i64,
        name: String,
        description: String,
    },
    MonitorRemoved {
        name: String,
    },
    Submap {
        name: String,
    },
    ActiveLayout {
        keyboard: String,
        layout: String,
    },
    Unknown {
        name: String,
        payload: String,
    },
}

pub fn parse_event(line: &str) -> Result<HyprlandEvent, EventParseError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (name, payload) = line.split_once(">>").unwrap_or((line, ""));

    match name {
        "workspacev2" => {
            let (id, workspace) = two(name, payload)?;
            Ok(HyprlandEvent::Workspace {
                id: integer(name, id)?,
                name: workspace.to_string(),
            })
        }
        "focusedmon" => {
            let (monitor, workspace) = two(name, payload)?;
            Ok(HyprlandEvent::FocusedMonitor {
                monitor: monitor.to_string(),
                workspace: workspace.to_string(),
            })
        }
        "createworkspacev2" => {
            let (id, workspace) = two(name, payload)?;
            Ok(HyprlandEvent::CreateWorkspace {
                id: integer(name, id)?,
                name: workspace.to_string(),
            })
        }
        "destroyworkspacev2" => {
            let (id, workspace) = two(name, payload)?;
            Ok(HyprlandEvent::DestroyWorkspace {
                id: integer(name, id)?,
                name: workspace.to_string(),
            })
        }
        "moveworkspacev2" => {
            let parts = fields(name, payload, 3)?;
            Ok(HyprlandEvent::MoveWorkspace {
                id: integer(name, parts[0])?,
                name: parts[1].to_string(),
                monitor: parts[2].to_string(),
            })
        }
        "renameworkspace" => {
            let (id, workspace) = two(name, payload)?;
            Ok(HyprlandEvent::RenameWorkspace {
                id: integer(name, id)?,
                name: workspace.to_string(),
            })
        }
        "activespecial" => {
            let (special, monitor) = match payload.split_once(',') {
                Some((special, monitor)) => (special, monitor),
                None => (payload, ""),
            };
            Ok(HyprlandEvent::ActiveSpecial {
                name: special.to_string(),
                monitor: monitor.to_string(),
            })
        }
        "openwindow" => {
            let parts = fields(name, payload, 4)?;
            Ok(HyprlandEvent::OpenWindow {
                address: crate::workspaces::model::normalize_address(parts[0]),
                workspace: parts[1].to_string(),
                class: parts[2].to_string(),
                title: parts[3].to_string(),
            })
        }
        "closewindow" => {
            let address = one(name, payload)?;
            Ok(HyprlandEvent::CloseWindow {
                address: crate::workspaces::model::normalize_address(address),
            })
        }
        "movewindowv2" => {
            let parts = fields(name, payload, 3)?;
            Ok(HyprlandEvent::MoveWindow {
                address: crate::workspaces::model::normalize_address(parts[0]),
                workspace: parts[1].to_string(),
                monitor: parts[2].to_string(),
            })
        }
        "urgent" => {
            let address = one(name, payload)?;
            Ok(HyprlandEvent::Urgent {
                address: crate::workspaces::model::normalize_address(address),
            })
        }
        "activewindowv2" => {
            let address = one(name, payload)?;
            Ok(HyprlandEvent::ActiveWindow {
                address: crate::workspaces::model::normalize_address(address),
            })
        }
        "changefloatingmode" => {
            let (address, floating) = two(name, payload)?;
            Ok(HyprlandEvent::ChangeFloatingMode {
                address: crate::workspaces::model::normalize_address(address),
                floating: boolean(name, floating)?,
            })
        }
        "fullscreen" => Ok(HyprlandEvent::Fullscreen {
            fullscreen: boolean(name, payload.trim())?,
        }),
        "windowtitlev2" => {
            let (address, title) = two(name, payload)?;
            Ok(HyprlandEvent::WindowTitle {
                address: crate::workspaces::model::normalize_address(address),
                title: title.to_string(),
            })
        }
        "configreloaded" => Ok(HyprlandEvent::ConfigReloaded),
        "monitoraddedv2" => {
            let parts = fields(name, payload, 3)?;
            Ok(HyprlandEvent::MonitorAdded {
                id: integer(name, parts[0])?,
                name: parts[1].to_string(),
                description: parts[2].to_string(),
            })
        }
        "monitorremoved" => Ok(HyprlandEvent::MonitorRemoved {
            name: one(name, payload)?.to_string(),
        }),
        "submap" => Ok(HyprlandEvent::Submap {
            name: payload.to_string(),
        }),
        "activelayout" => {
            let (keyboard, layout) = payload.split_once(',').unwrap_or((payload, ""));
            Ok(HyprlandEvent::ActiveLayout {
                keyboard: keyboard.to_string(),
                layout: layout.to_string(),
            })
        }
        _ => Ok(HyprlandEvent::Unknown {
            name: name.to_string(),
            payload: payload.to_string(),
        }),
    }
}

fn one<'a>(event: &str, payload: &'a str) -> Result<&'a str, EventParseError> {
    if payload.is_empty() {
        return Err(EventParseError::Fields {
            event: event.to_string(),
            expected: 1,
            got: 0,
        });
    }
    Ok(payload)
}

fn two<'a>(event: &str, payload: &'a str) -> Result<(&'a str, &'a str), EventParseError> {
    payload
        .split_once(',')
        .ok_or_else(|| EventParseError::Fields {
            event: event.to_string(),
            expected: 2,
            got: payload.split(',').count(),
        })
}

fn fields<'a>(
    event: &str,
    payload: &'a str,
    count: usize,
) -> Result<Vec<&'a str>, EventParseError> {
    let parts: Vec<&str> = payload.splitn(count, ',').collect();
    if parts.len() != count || parts.first().is_some_and(|part| part.is_empty()) {
        return Err(EventParseError::Fields {
            event: event.to_string(),
            expected: count,
            got: parts.iter().filter(|part| !part.is_empty()).count(),
        });
    }
    Ok(parts)
}

fn integer(event: &str, value: &str) -> Result<i64, EventParseError> {
    value.parse().map_err(|_| EventParseError::Invalid {
        event: event.to_string(),
        value: value.to_string(),
    })
}

fn boolean(event: &str, value: &str) -> Result<bool, EventParseError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        other => Err(EventParseError::Invalid {
            event: event.to_string(),
            value: other.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_event_splits_id_and_name() {
        assert_eq!(
            parse_event("workspacev2>>1,1").unwrap(),
            HyprlandEvent::Workspace {
                id: 1,
                name: "1".to_string()
            }
        );
    }

    #[test]
    fn focused_monitor_event_splits_monitor_and_workspace() {
        assert_eq!(
            parse_event("focusedmon>>DP-1,1").unwrap(),
            HyprlandEvent::FocusedMonitor {
                monitor: "DP-1".to_string(),
                workspace: "1".to_string()
            }
        );
    }

    #[test]
    fn workspace_lifecycle_events_parse() {
        assert_eq!(
            parse_event("createworkspacev2>>2,2").unwrap(),
            HyprlandEvent::CreateWorkspace {
                id: 2,
                name: "2".to_string()
            }
        );
        assert_eq!(
            parse_event("destroyworkspacev2>>2,2").unwrap(),
            HyprlandEvent::DestroyWorkspace {
                id: 2,
                name: "2".to_string()
            }
        );
        assert_eq!(
            parse_event("moveworkspacev2>>2,2,DP-2").unwrap(),
            HyprlandEvent::MoveWorkspace {
                id: 2,
                name: "2".to_string(),
                monitor: "DP-2".to_string()
            }
        );
        assert_eq!(
            parse_event("renameworkspace>>2,web").unwrap(),
            HyprlandEvent::RenameWorkspace {
                id: 2,
                name: "web".to_string()
            }
        );
    }

    #[test]
    fn activespecial_accepts_an_empty_payload() {
        assert_eq!(
            parse_event("activespecial>>").unwrap(),
            HyprlandEvent::ActiveSpecial {
                name: String::new(),
                monitor: String::new()
            }
        );
        assert_eq!(
            parse_event("activespecial>>special:magic,DP-1").unwrap(),
            HyprlandEvent::ActiveSpecial {
                name: "special:magic".to_string(),
                monitor: "DP-1".to_string()
            }
        );
    }

    #[test]
    fn openwindow_keeps_commas_in_the_title() {
        assert_eq!(
            parse_event("openwindow>>0xABC123,1,kitty,Terminal, draft").unwrap(),
            HyprlandEvent::OpenWindow {
                address: "abc123".to_string(),
                workspace: "1".to_string(),
                class: "kitty".to_string(),
                title: "Terminal, draft".to_string()
            }
        );
    }

    #[test]
    fn window_events_normalize_addresses() {
        assert_eq!(
            parse_event("closewindow>>0xABC").unwrap(),
            HyprlandEvent::CloseWindow {
                address: "abc".to_string()
            }
        );
        assert_eq!(
            parse_event("urgent>>0xABC").unwrap(),
            HyprlandEvent::Urgent {
                address: "abc".to_string()
            }
        );
        assert_eq!(
            parse_event("activewindowv2>>0xABC").unwrap(),
            HyprlandEvent::ActiveWindow {
                address: "abc".to_string()
            }
        );
        assert_eq!(
            parse_event("movewindowv2>>0xABC,2,DP-1").unwrap(),
            HyprlandEvent::MoveWindow {
                address: "abc".to_string(),
                workspace: "2".to_string(),
                monitor: "DP-1".to_string()
            }
        );
    }

    #[test]
    fn boolean_events_accept_zero_and_one() {
        assert_eq!(
            parse_event("changefloatingmode>>0xABC,1").unwrap(),
            HyprlandEvent::ChangeFloatingMode {
                address: "abc".to_string(),
                floating: true
            }
        );
        assert_eq!(
            parse_event("fullscreen>>0").unwrap(),
            HyprlandEvent::Fullscreen { fullscreen: false }
        );
    }

    #[test]
    fn title_event_keeps_commas() {
        assert_eq!(
            parse_event("windowtitlev2>>0xABC,one, two").unwrap(),
            HyprlandEvent::WindowTitle {
                address: "abc".to_string(),
                title: "one, two".to_string()
            }
        );
    }

    #[test]
    fn payloadless_events_parse() {
        assert_eq!(
            parse_event("configreloaded>>").unwrap(),
            HyprlandEvent::ConfigReloaded
        );
        assert_eq!(
            parse_event("submap>>").unwrap(),
            HyprlandEvent::Submap {
                name: String::new()
            }
        );
    }

    #[test]
    fn monitor_events_parse() {
        assert_eq!(
            parse_event("monitoraddedv2>>1,DP-1,Dell Inc., monitor").unwrap(),
            HyprlandEvent::MonitorAdded {
                id: 1,
                name: "DP-1".to_string(),
                description: "Dell Inc., monitor".to_string()
            }
        );
        assert_eq!(
            parse_event("monitorremoved>>DP-1").unwrap(),
            HyprlandEvent::MonitorRemoved {
                name: "DP-1".to_string()
            }
        );
    }

    #[test]
    fn unknown_events_are_preserved_without_an_error() {
        assert_eq!(
            parse_event("somefutureevent>>a,b").unwrap(),
            HyprlandEvent::Unknown {
                name: "somefutureevent".to_string(),
                payload: "a,b".to_string()
            }
        );
    }

    #[test]
    fn malformed_payloads_are_errors() {
        assert!(matches!(
            parse_event("workspacev2>>1"),
            Err(EventParseError::Fields { .. })
        ));
        assert!(matches!(
            parse_event("workspacev2>>notanumber,1"),
            Err(EventParseError::Invalid { .. })
        ));
        assert!(matches!(
            parse_event("closewindow>>"),
            Err(EventParseError::Fields { .. })
        ));
        assert!(matches!(
            parse_event("fullscreen>>maybe"),
            Err(EventParseError::Invalid { .. })
        ));
    }

    #[test]
    fn a_line_without_a_separator_is_unknown_not_a_panic() {
        assert!(matches!(
            parse_event("garbage"),
            Ok(HyprlandEvent::Unknown { .. })
        ));
    }
}
