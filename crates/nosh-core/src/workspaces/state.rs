use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    normalize_address, sort_workspaces, WorkspaceModel, WorkspaceMonitor, WorkspaceSnapshot,
};

/// The authoritative workspace list, focus, and urgent set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceState {
    pub workspaces: Vec<WorkspaceModel>,
    pub monitors: Vec<WorkspaceMonitor>,
    pub focused_workspace: Option<String>,
    pub urgent: BTreeSet<String>,
    client_workspaces: BTreeMap<String, String>,
}

impl WorkspaceState {
    /// Replaces the list from a fresh read while keeping urgency for workspaces
    /// that still exist. A focused workspace never stays urgent.
    pub fn replace(
        &mut self,
        mut workspaces: Vec<WorkspaceModel>,
        mut monitors: Vec<WorkspaceMonitor>,
        clients: BTreeMap<String, String>,
        focused: Option<String>,
    ) {
        sort_workspaces(&mut workspaces);
        monitors.sort_by_key(|monitor| monitor.id);

        let ids: BTreeSet<&str> = workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect();
        let focused_key = focused.as_deref();
        self.urgent
            .retain(|key| ids.contains(key.as_str()) && Some(key.as_str()) != focused_key);

        for workspace in &mut workspaces {
            workspace.focused = focused_key == Some(workspace.id.as_str());
            workspace.urgent = self.urgent.contains(&workspace.id);
        }

        self.workspaces = workspaces;
        self.monitors = monitors;
        self.focused_workspace = focused;
        self.client_workspaces = clients;
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            workspaces: self.workspaces.clone(),
            monitors: self.monitors.clone(),
            focused_workspace: self.focused_workspace.clone(),
        }
    }

    /// Marks the client's workspace urgent when the client is known and its
    /// workspace is not focused. Returns the workspace key on a change.
    pub fn apply_urgent(&mut self, address: &str) -> Option<String> {
        let address = normalize_address(address);
        let workspace = self.client_workspaces.get(&address)?.clone();

        if self.focused_workspace.as_deref() == Some(workspace.as_str()) {
            return None;
        }
        if !self.workspaces.iter().any(|item| item.id == workspace) {
            return None;
        }
        if !self.urgent.insert(workspace.clone()) {
            return None;
        }

        if let Some(model) = self.workspaces.iter_mut().find(|item| item.id == workspace) {
            model.urgent = true;
        }
        Some(workspace)
    }

    /// Sets focus by workspace key, clears its urgent flag, and returns whether
    /// focus actually changed.
    pub fn set_focused(&mut self, focused: Option<String>) -> bool {
        if self.focused_workspace == focused {
            return false;
        }

        self.focused_workspace = focused.clone();
        if let Some(id) = &focused {
            self.urgent.remove(id);
        }
        for workspace in &mut self.workspaces {
            workspace.focused = focused.as_deref() == Some(workspace.id.as_str());
            if workspace.focused {
                workspace.urgent = false;
            }
        }

        true
    }

    pub fn remove_workspace(&mut self, id: &str) {
        self.workspaces.retain(|workspace| workspace.id != id);
        self.urgent.remove(id);
        if self.focused_workspace.as_deref() == Some(id) {
            self.focused_workspace = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::WorkspaceCapabilities;

    use super::*;

    fn workspace(id: &str) -> WorkspaceModel {
        WorkspaceModel {
            id: id.to_string(),
            name: id.to_string(),
            monitor: Some("DP-1".to_string()),
            special: false,
            has_fullscreen: false,
            focused: false,
            urgent: false,
            capabilities: WorkspaceCapabilities::all(),
        }
    }

    fn clients(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(address, workspace)| (address.to_string(), workspace.to_string()))
            .collect()
    }

    #[test]
    fn replace_sets_focus_flags() {
        let mut state = WorkspaceState::default();

        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![WorkspaceMonitor {
                id: 0,
                connector: "DP-1".to_string(),
                focused: true,
            }],
            clients(&[]),
            Some("2".to_string()),
        );

        assert_eq!(state.focused_workspace.as_deref(), Some("2"));
        assert!(!state.workspaces[0].focused);
        assert!(state.workspaces[1].focused);
    }

    #[test]
    fn urgent_is_marked_only_for_a_known_client_on_an_unfocused_workspace() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );

        assert_eq!(state.apply_urgent("0xABC").as_deref(), Some("2"));
        assert!(state.urgent.contains("2"));
        assert!(state.workspaces[1].urgent);
        assert_eq!(state.apply_urgent("0xABC"), None);
        assert_eq!(state.apply_urgent("0xDEF"), None);
    }

    #[test]
    fn urgent_is_ignored_for_the_focused_workspace() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1")],
            vec![],
            clients(&[("abc", "1")]),
            Some("1".to_string()),
        );

        assert_eq!(state.apply_urgent("abc"), None);
        assert!(state.urgent.is_empty());
    }

    #[test]
    fn urgent_survives_a_resync() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );
        state.apply_urgent("abc");

        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );

        assert!(state.urgent.contains("2"));
        assert!(
            state
                .workspaces
                .iter()
                .find(|work| work.id == "2")
                .unwrap()
                .urgent
        );
    }

    #[test]
    fn focus_clears_urgent() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );
        state.apply_urgent("abc");

        assert!(state.set_focused(Some("2".to_string())));

        assert!(state.urgent.is_empty());
        assert!(!state.workspaces[1].urgent);
        assert!(state.workspaces[1].focused);
    }

    #[test]
    fn removing_a_workspace_clears_its_urgent_key() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );
        state.apply_urgent("abc");

        state.remove_workspace("2");

        assert!(state.urgent.is_empty());
        assert!(state.workspaces.iter().all(|work| work.id != "2"));
    }

    #[test]
    fn a_removed_workspace_drops_its_urgent_key_on_the_next_resync() {
        let mut state = WorkspaceState::default();
        state.replace(
            vec![workspace("1"), workspace("2")],
            vec![],
            clients(&[("abc", "2")]),
            Some("1".to_string()),
        );
        state.apply_urgent("abc");

        state.replace(
            vec![workspace("1")],
            vec![],
            clients(&[]),
            Some("1".to_string()),
        );

        assert!(state.urgent.is_empty());
    }
}
