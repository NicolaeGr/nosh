//! Dispatch formatting for the two Hyprland config providers.
//!
//! Provider detection uses `configProvider` from `j/status` when it is present.
//! Older or stripped responses fall back to a version table.

/// Hyprland releases at or above this version use Lua when `j/status` has no
/// `configProvider`. Change the cutoff here.
pub const LUA_PROVIDER_MIN_VERSION: Version = Version {
    major: 0,
    minor: 50,
    patch: 0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigProvider {
    Hyprlang,
    Lua,
}

impl ConfigProvider {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "lua" => Some(Self::Lua),
            "hyprlang" => Some(Self::Hyprlang),
            _ => None,
        }
    }

    pub fn from_version(tag: &str) -> Self {
        match Version::parse(tag) {
            Some(version) if version >= LUA_PROVIDER_MIN_VERSION => Self::Lua,
            _ => Self::Hyprlang,
        }
    }
}

/// Chooses the provider from `j/status`, then the version tag, then hyprlang.
pub fn detect_provider(config_provider: Option<&str>, version_tag: Option<&str>) -> ConfigProvider {
    config_provider
        .and_then(ConfigProvider::parse)
        .or_else(|| version_tag.map(ConfigProvider::from_version))
        .unwrap_or(ConfigProvider::Hyprlang)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn parse(tag: &str) -> Option<Self> {
        let tag = tag.trim().trim_start_matches('v');
        let core = tag.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;

        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch<'a> {
    FocusWorkspace(&'a str),
    FocusClient(&'a str),
    FocusMonitor(i32),
    CloseClient(&'a str),
    MoveClientToWorkspace {
        address: &'a str,
        workspace: &'a str,
    },
    ToggleFloating(&'a str),
    MoveWorkspaceToMonitor {
        workspace: &'a str,
        monitor: &'a str,
    },
    MoveCursor {
        x: i32,
        y: i32,
    },
    Exec(&'a str),
}

pub fn format_dispatch(provider: ConfigProvider, action: Dispatch<'_>) -> String {
    match provider {
        ConfigProvider::Hyprlang => format_hyprlang(action),
        ConfigProvider::Lua => format_lua(action),
    }
}

fn format_hyprlang(action: Dispatch<'_>) -> String {
    match action {
        Dispatch::FocusWorkspace(workspace) => format!("dispatch workspace {workspace}"),
        Dispatch::FocusClient(address) => {
            format!(
                "dispatch focuswindow address:0x{}",
                strip_address_prefix(address)
            )
        }
        Dispatch::FocusMonitor(monitor) => format!("dispatch focusmonitor {monitor}"),
        Dispatch::CloseClient(address) => {
            format!(
                "dispatch closewindow address:0x{}",
                strip_address_prefix(address)
            )
        }
        Dispatch::MoveClientToWorkspace { address, workspace } => format!(
            "dispatch movetoworkspacesilent {workspace},address:0x{}",
            strip_address_prefix(address)
        ),
        Dispatch::ToggleFloating(address) => {
            format!(
                "dispatch togglefloating address:0x{}",
                strip_address_prefix(address)
            )
        }
        Dispatch::MoveWorkspaceToMonitor { workspace, monitor } => {
            format!("dispatch moveworkspacetomonitor {workspace} {monitor}")
        }
        Dispatch::MoveCursor { x, y } => format!("dispatch movecursor {x} {y}"),
        Dispatch::Exec(command) => format!("dispatch exec {command}"),
    }
}

fn format_lua(action: Dispatch<'_>) -> String {
    match action {
        Dispatch::FocusWorkspace(workspace) => format!(
            "dispatch hl.dsp.focus({{ workspace = \"{}\" }})",
            escape_lua(workspace)
        ),
        Dispatch::FocusClient(address) => format!(
            "dispatch hl.dsp.focus({{ window = \"address:0x{}\" }})",
            strip_address_prefix(address)
        ),
        Dispatch::FocusMonitor(monitor) => {
            format!("dispatch hl.dsp.focus({{ monitor = {monitor} }})")
        }
        Dispatch::CloseClient(address) => format!(
            "dispatch hl.dsp.window.close({{ window = \"address:0x{}\" }})",
            strip_address_prefix(address)
        ),
        Dispatch::MoveClientToWorkspace { address, workspace } => format!(
            "dispatch hl.dsp.window.move({{ window = \"address:0x{}\", workspace = {}, follow = false }})",
            strip_address_prefix(address),
            lua_value(workspace)
        ),
        Dispatch::ToggleFloating(address) => format!(
            "dispatch hl.dsp.window.float({{ window = \"address:0x{}\" }})",
            strip_address_prefix(address)
        ),
        Dispatch::MoveWorkspaceToMonitor {
            workspace,
            monitor,
        } => format!(
            "dispatch hl.dsp.workspace.move({{ workspace = {}, monitor = {} }})",
            lua_value(workspace),
            lua_value(monitor)
        ),
        Dispatch::MoveCursor { x, y } => {
            format!("dispatch hl.dsp.cursor.move({{ x = {x}, y = {y} }})")
        }
        Dispatch::Exec(command) => {
            format!("dispatch hl.dsp.exec_cmd(\"{}\")", escape_lua(command))
        }
    }
}

fn strip_address_prefix(value: &str) -> &str {
    value.strip_prefix("0x").unwrap_or(value)
}

/// Lua accepts a bare number for numeric ids and needs a string otherwise.
fn lua_value(value: &str) -> String {
    if value.parse::<i64>().is_ok() {
        value.to_string()
    } else {
        format!("\"{}\"", escape_lua(value))
    }
}

fn escape_lua(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprlang_dispatch_strings_match_the_spec() {
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::FocusWorkspace("3")),
            "dispatch workspace 3"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::FocusClient("0xABC")),
            "dispatch focuswindow address:0xABC"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::FocusMonitor(1)),
            "dispatch focusmonitor 1"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::CloseClient("abc")),
            "dispatch closewindow address:0xabc"
        );
        assert_eq!(
            format_dispatch(
                ConfigProvider::Hyprlang,
                Dispatch::MoveClientToWorkspace {
                    address: "abc",
                    workspace: "2",
                }
            ),
            "dispatch movetoworkspacesilent 2,address:0xabc"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::ToggleFloating("abc")),
            "dispatch togglefloating address:0xabc"
        );
        assert_eq!(
            format_dispatch(
                ConfigProvider::Hyprlang,
                Dispatch::MoveWorkspaceToMonitor {
                    workspace: "2",
                    monitor: "DP-1",
                }
            ),
            "dispatch moveworkspacetomonitor 2 DP-1"
        );
        assert_eq!(
            format_dispatch(
                ConfigProvider::Hyprlang,
                Dispatch::MoveCursor { x: 10, y: 20 }
            ),
            "dispatch movecursor 10 20"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Hyprlang, Dispatch::Exec("kitty")),
            "dispatch exec kitty"
        );
    }

    #[test]
    fn lua_dispatch_strings_match_the_spec() {
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::FocusWorkspace("3")),
            "dispatch hl.dsp.focus({ workspace = \"3\" })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::FocusClient("0xABC")),
            "dispatch hl.dsp.focus({ window = \"address:0xABC\" })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::FocusMonitor(1)),
            "dispatch hl.dsp.focus({ monitor = 1 })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::CloseClient("abc")),
            "dispatch hl.dsp.window.close({ window = \"address:0xabc\" })"
        );
        assert_eq!(
            format_dispatch(
                ConfigProvider::Lua,
                Dispatch::MoveClientToWorkspace {
                    address: "abc",
                    workspace: "2",
                }
            ),
            "dispatch hl.dsp.window.move({ window = \"address:0xabc\", workspace = 2, follow = false })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::ToggleFloating("abc")),
            "dispatch hl.dsp.window.float({ window = \"address:0xabc\" })"
        );
        assert_eq!(
            format_dispatch(
                ConfigProvider::Lua,
                Dispatch::MoveWorkspaceToMonitor {
                    workspace: "2",
                    monitor: "DP-1",
                }
            ),
            "dispatch hl.dsp.workspace.move({ workspace = 2, monitor = \"DP-1\" })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::MoveCursor { x: 10, y: 20 }),
            "dispatch hl.dsp.cursor.move({ x = 10, y = 20 })"
        );
        assert_eq!(
            format_dispatch(ConfigProvider::Lua, Dispatch::Exec("kitty")),
            "dispatch hl.dsp.exec_cmd(\"kitty\")"
        );
    }

    #[test]
    fn a_leading_address_prefix_is_not_doubled() {
        let hyprlang = format_dispatch(ConfigProvider::Hyprlang, Dispatch::FocusClient("0xabc"));
        let lua = format_dispatch(ConfigProvider::Lua, Dispatch::FocusClient("0xabc"));

        assert!(!hyprlang.contains("0x0x"));
        assert!(!lua.contains("0x0x"));
    }

    #[test]
    fn lua_strings_are_escaped() {
        let dispatch = format_dispatch(
            ConfigProvider::Lua,
            Dispatch::Exec("notify-send \"hi\" C:\\tmp"),
        );

        assert_eq!(
            dispatch,
            "dispatch hl.dsp.exec_cmd(\"notify-send \\\"hi\\\" C:\\\\tmp\")"
        );
    }

    #[test]
    fn named_workspaces_are_quoted_in_lua_but_numeric_ones_are_not() {
        let numeric = format_dispatch(
            ConfigProvider::Lua,
            Dispatch::MoveWorkspaceToMonitor {
                workspace: "3",
                monitor: "DP-1",
            },
        );
        let named = format_dispatch(
            ConfigProvider::Lua,
            Dispatch::MoveWorkspaceToMonitor {
                workspace: "browser",
                monitor: "DP-1",
            },
        );

        assert!(numeric.contains("workspace = 3"));
        assert!(named.contains("workspace = \"browser\""));
    }

    #[test]
    fn status_provider_wins_over_the_version() {
        assert_eq!(
            detect_provider(Some("hyprlang"), Some("v0.60.0")),
            ConfigProvider::Hyprlang
        );
        assert_eq!(
            detect_provider(Some("lua"), Some("v0.40.0")),
            ConfigProvider::Lua
        );
    }

    #[test]
    fn an_unknown_status_provider_falls_back_to_the_version() {
        assert_eq!(
            detect_provider(Some("something"), Some("v0.60.0")),
            ConfigProvider::Lua
        );
        assert_eq!(
            detect_provider(None, Some("v0.40.0")),
            ConfigProvider::Hyprlang
        );
    }

    #[test]
    fn a_missing_provider_and_version_defaults_to_hyprlang() {
        assert_eq!(detect_provider(None, None), ConfigProvider::Hyprlang);
        assert_eq!(
            detect_provider(None, Some("not-a-version")),
            ConfigProvider::Hyprlang
        );
    }

    #[test]
    fn versions_parse_with_or_without_a_prefix() {
        assert_eq!(
            Version::parse("v0.50.1"),
            Some(Version {
                major: 0,
                minor: 50,
                patch: 1
            })
        );
        assert_eq!(
            Version::parse("0.50"),
            Some(Version {
                major: 0,
                minor: 50,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("0.50.1-abc+def"),
            Some(Version {
                major: 0,
                minor: 50,
                patch: 1
            })
        );
        assert_eq!(Version::parse("unknown"), None);
    }
}
