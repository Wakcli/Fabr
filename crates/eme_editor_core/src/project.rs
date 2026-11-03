use core::cmp::Reverse;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Projects kept in the recent list before the oldest is dropped.
pub const MAX_RECENT: usize = 20;

/// Stable identifier for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub u64);

/// A project registered with the editor hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Identifier of the project.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// Project root directory.
    pub root: PathBuf,
    /// Unix timestamp of the last open, if it was ever opened.
    pub last_opened: Option<u64>,
    /// Engine version the project was last saved with.
    pub engine_version: String,
}

impl Project {
    fn matches(&self, needle: &str) -> bool {
        self.name.to_lowercase().contains(needle)
            || self.root.to_string_lossy().to_lowercase().contains(needle)
    }
}

/// Recent projects, the current selection, and the hub filter.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HubState {
    /// Known projects, most recently opened first.
    pub recent: Vec<Project>,
    /// Currently selected project.
    pub selected: Option<ProjectId>,
    /// Case-insensitive filter applied to names and roots.
    pub filter: String,
}

impl HubState {
    /// An empty hub.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a new project or replaces an existing one, then re-sorts and caps the list.
    pub fn add_or_touch(&mut self, project: Project) {
        match self.recent.iter_mut().find(|p| p.id == project.id) {
            Some(slot) => *slot = project,
            None => self.recent.push(project),
        }
        self.recent.sort_by_key(|p| Reverse(p.last_opened));
        self.recent.truncate(MAX_RECENT);
    }

    /// Drops a project and clears the selection if it pointed at that project.
    pub fn remove(&mut self, id: ProjectId) {
        self.recent.retain(|p| p.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
    }

    /// Projects passing the current filter, in recent order.
    pub fn filtered(&self) -> Vec<&Project> {
        let needle = self.filter.to_lowercase();
        if needle.is_empty() {
            return self.recent.iter().collect();
        }
        self.recent.iter().filter(|p| p.matches(&needle)).collect()
    }

    /// Selects the next filtered project, wrapping at the end.
    pub fn select_next(&mut self) {
        let list = self.filtered();
        if list.is_empty() {
            self.selected = None;
            return;
        }
        let idx = self
            .selected
            .and_then(|id| list.iter().position(|p| p.id == id));
        let next = idx.map_or(0, |i| (i + 1) % list.len());
        self.selected = Some(list[next].id);
    }

    /// Selects the previous filtered project, wrapping at the start.
    pub fn select_prev(&mut self) {
        let list = self.filtered();
        if list.is_empty() {
            self.selected = None;
            return;
        }
        let len = list.len();
        let idx = self
            .selected
            .and_then(|id| list.iter().position(|p| p.id == id));
        let prev = idx.map_or(len - 1, |i| (i + len - 1) % len);
        self.selected = Some(list[prev].id);
    }

    /// The selected project, if it is still in the recent list.
    pub fn selected_project(&self) -> Option<&Project> {
        let id = self.selected?;
        self.recent.iter().find(|p| p.id == id)
    }

    /// Replaces the filter text.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: u64, name: &str, root: &str, opened: Option<u64>) -> Project {
        Project {
            id: ProjectId(id),
            name: name.into(),
            root: PathBuf::from(root),
            last_opened: opened,
            engine_version: "0.20.0".into(),
        }
    }

    #[test]
    fn add_or_touch_replaces_and_sorts() {
        let mut hub = HubState::new();
        hub.add_or_touch(project(1, "Alpha", "/a", Some(10)));
        hub.add_or_touch(project(2, "Beta", "/b", Some(20)));
        hub.add_or_touch(project(1, "Alpha renamed", "/a", Some(30)));

        assert_eq!(hub.recent.len(), 2);
        assert_eq!(hub.recent[0].id, ProjectId(1));
        assert_eq!(hub.recent[0].name, "Alpha renamed");
        assert_eq!(hub.recent[1].id, ProjectId(2));
    }

    #[test]
    fn recent_is_capped_and_ordered() {
        let mut hub = HubState::new();
        for id in 1..=MAX_RECENT as u64 + 5 {
            hub.add_or_touch(project(id, "P", "/p", Some(id)));
        }

        assert_eq!(hub.recent.len(), MAX_RECENT);
        let newest = MAX_RECENT as u64 + 5;
        assert_eq!(hub.recent[0].id, ProjectId(newest));
        assert_eq!(hub.recent[MAX_RECENT - 1].id, ProjectId(newest - 19));
    }

    #[test]
    fn filter_matches_name_and_root_case_insensitively() {
        let mut hub = HubState::new();
        hub.add_or_touch(project(1, "Ember Game", "/work/ember", Some(1)));
        hub.add_or_touch(project(2, "Other", "/work/Fire", Some(2)));

        hub.set_filter("EMBER");
        assert_eq!(hub.filtered().len(), 1);
        assert_eq!(hub.filtered()[0].id, ProjectId(1));

        hub.set_filter("fire");
        assert_eq!(hub.filtered().len(), 1);
        assert_eq!(hub.filtered()[0].id, ProjectId(2));

        hub.set_filter("nothing");
        assert!(hub.filtered().is_empty());
    }

    #[test]
    fn select_walks_filtered_list_and_wraps() {
        let mut hub = HubState::new();
        hub.add_or_touch(project(1, "a", "/a", Some(30)));
        hub.add_or_touch(project(2, "b", "/b", Some(20)));
        hub.add_or_touch(project(3, "c", "/c", Some(10)));
        hub.set_filter("");

        hub.select_next();
        assert_eq!(hub.selected, Some(ProjectId(1)));
        hub.select_next();
        assert_eq!(hub.selected, Some(ProjectId(2)));
        hub.select_prev();
        assert_eq!(hub.selected, Some(ProjectId(1)));
        hub.select_prev();
        assert_eq!(hub.selected, Some(ProjectId(3)));
        hub.select_next();
        assert_eq!(hub.selected, Some(ProjectId(1)));
        assert_eq!(hub.selected_project().map(|p| p.id), Some(ProjectId(1)));
    }

    #[test]
    fn remove_clears_selection_and_filtered_selection_skips_hidden() {
        let mut hub = HubState::new();
        hub.add_or_touch(project(1, "Alpha", "/a", Some(10)));
        hub.add_or_touch(project(2, "Beta", "/b", Some(20)));
        hub.selected = Some(ProjectId(2));

        hub.remove(ProjectId(2));
        assert_eq!(hub.recent.len(), 1);
        assert!(hub.selected.is_none());

        hub.set_filter("zzz");
        hub.select_next();
        assert!(hub.selected.is_none());
        assert!(hub.selected_project().is_none());
    }
}
