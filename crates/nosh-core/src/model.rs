use std::cmp::Ordering;

use bitflags::bitflags;

bitflags! {
    /// Capabilities a workspace backend provides.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WorkspaceCapabilities: u8 {
        const READ = 1 << 0;
        const WATCH = 1 << 1;
        const ACTIVATE = 1 << 2;
        const MOVE = 1 << 3;
        const URGENT = 1 << 4;
        const SPECIAL = 1 << 5;
    }
}

impl WorkspaceCapabilities {
    /// The capabilities the bar needs to render workspaces.
    pub const BAR: Self = Self::READ.union(Self::WATCH);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceModel {
    /// Stable key. The Hyprland backend uses the numeric id as a decimal string.
    pub id: String,
    pub name: String,
    /// Connector name when known.
    pub monitor: Option<String>,
    pub special: bool,
    pub has_fullscreen: bool,
    pub focused: bool,
    pub urgent: bool,
    pub capabilities: WorkspaceCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorModel {
    pub id: i32,
    pub connector: String,
    pub focused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub workspaces: Vec<WorkspaceModel>,
    pub monitors: Vec<MonitorModel>,
    pub focused_workspace: Option<String>,
}

impl WorkspaceSnapshot {
    pub fn workspace(&self, id: &str) -> Option<&WorkspaceModel> {
        self.workspaces.iter().find(|workspace| workspace.id == id)
    }
}

/// Canonical client address: no `0x` prefix, lower case.
pub fn normalize_address(address: &str) -> String {
    address
        .strip_prefix("0x")
        .unwrap_or(address)
        .to_ascii_lowercase()
}

/// Sorts numeric ids numerically and named workspaces by name.
pub fn sort_workspaces(workspaces: &mut [WorkspaceModel]) {
    workspaces.sort_by(
        |left, right| match (left.id.parse::<i64>(), right.id.parse::<i64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => left.name.cmp(&right.name),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(id: &str, name: &str) -> WorkspaceModel {
        WorkspaceModel {
            id: id.to_string(),
            name: name.to_string(),
            monitor: None,
            special: false,
            has_fullscreen: false,
            focused: false,
            urgent: false,
            capabilities: WorkspaceCapabilities::empty(),
        }
    }

    #[test]
    fn addresses_lose_the_prefix_and_case() {
        assert_eq!(normalize_address("0xABC123"), "abc123");
        assert_eq!(normalize_address("abc123"), "abc123");
    }

    #[test]
    fn numeric_ids_sort_numerically() {
        let mut workspaces = vec![
            workspace("10", "10"),
            workspace("2", "2"),
            workspace("1", "1"),
        ];

        sort_workspaces(&mut workspaces);

        let ids: Vec<&str> = workspaces.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "10"]);
    }

    #[test]
    fn named_workspaces_sort_after_numeric_ones() {
        let mut workspaces = vec![
            workspace("browser", "browser"),
            workspace("2", "2"),
            workspace("editor", "editor"),
        ];

        sort_workspaces(&mut workspaces);

        let ids: Vec<&str> = workspaces.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["2", "browser", "editor"]);
    }
}
