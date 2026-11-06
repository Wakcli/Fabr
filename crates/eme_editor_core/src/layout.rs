use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Smallest fraction of the axis a split side may occupy.
pub const MIN_RATIO: f32 = 0.05;
/// Largest fraction of the axis a split side may occupy.
pub const MAX_RATIO: f32 = 0.95;

/// Split direction of a layout node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// Children sit side by side.
    Horizontal,
    /// Children are stacked.
    Vertical,
}

/// Side of a dock a moved tab can take when it becomes a new split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Left of the target dock.
    Left,
    /// Right of the target dock.
    Right,
    /// Above the target dock.
    Top,
    /// Below the target dock.
    Bottom,
}

impl Edge {
    /// The split axis and whether the moved tab becomes the first child.
    pub fn split(self) -> (Axis, bool) {
        match self {
            Edge::Left => (Axis::Horizontal, true),
            Edge::Top => (Axis::Vertical, true),
            Edge::Right => (Axis::Horizontal, false),
            Edge::Bottom => (Axis::Vertical, false),
        }
    }
}

/// Stable identifier for a panel tab or a dock leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PanelId(pub u32);

/// What a panel displays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelKind {
    /// Scene tree.
    Hierarchy,
    /// Viewport.
    Scene,
    /// Component editor.
    Inspector,
    /// File and asset browser.
    Project,
    /// Log output.
    Console,
    /// Editor-provided panel.
    Custom(String),
}

/// One tab inside a dock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelTab {
    /// Identifier of the tab.
    pub id: PanelId,
    /// Tab contents.
    pub kind: PanelKind,
}

/// Binary tree of splits and dock leaves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Layout {
    /// Two children sharing an axis.
    Split {
        /// Split direction.
        axis: Axis,
        /// Fraction of the axis given to `first`.
        ratio: f32,
        /// Top or left child.
        first: Box<Layout>,
        /// Bottom or right child.
        second: Box<Layout>,
    },
    /// A leaf dock holding one or more tabs.
    Panel {
        /// Identifier of the dock, used as the drop target.
        id: PanelId,
        /// Tabs in strip order.
        tabs: Vec<PanelTab>,
        /// Index of the visible tab.
        active: usize,
    },
}

fn set_ratio_inner(layout: &mut Layout, index: usize, ratio: f32, cursor: &mut usize) -> bool {
    match layout {
        Layout::Split {
            ratio: slot,
            first,
            second,
            ..
        } => {
            let here = *cursor;
            *cursor += 1;
            if here == index {
                *slot = ratio;
                return true;
            }
            set_ratio_inner(first, index, ratio, cursor)
                || set_ratio_inner(second, index, ratio, cursor)
        }
        Layout::Panel { .. } => false,
    }
}

fn ratio_inner(layout: &Layout, index: usize, cursor: &mut usize) -> Option<f32> {
    match layout {
        Layout::Split {
            ratio,
            first,
            second,
            ..
        } => {
            let here = *cursor;
            *cursor += 1;
            if here == index {
                return Some(*ratio);
            }
            ratio_inner(first, index, cursor).or_else(|| ratio_inner(second, index, cursor))
        }
        Layout::Panel { .. } => None,
    }
}


impl Layout {
    /// The eight-region arrangement used when no layout is saved.
    pub fn default_editor() -> Self {
        let panel = |dock: u32, tab: u32, kind: PanelKind| Layout::Panel {
            id: PanelId(dock),
            tabs: vec![PanelTab {
                id: PanelId(tab),
                kind,
            }],
            active: 0,
        };
        let split = |axis: Axis, ratio: f32, first: Layout, second: Layout| Layout::Split {
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        };

        let left = split(
            Axis::Vertical,
            0.82,
            panel(1, 9, PanelKind::Hierarchy),
            panel(2, 10, PanelKind::Custom("Outline".into())),
        );
        let bottom = Layout::Panel {
            id: PanelId(4),
            tabs: vec![
                PanelTab {
                    id: PanelId(12),
                    kind: PanelKind::Project,
                },
                PanelTab {
                    id: PanelId(13),
                    kind: PanelKind::Console,
                },
            ],
            active: 0,
        };
        let center = split(Axis::Vertical, 0.70, panel(3, 11, PanelKind::Scene), bottom);
        let right = split(
            Axis::Vertical,
            0.62,
            panel(6, 14, PanelKind::Inspector),
            panel(7, 15, PanelKind::Custom("Stats".into())),
        );

        split(
            Axis::Vertical,
            0.94,
            split(
                Axis::Horizontal,
                0.18,
                left,
                split(Axis::Horizontal, 0.82, center, right),
            ),
            panel(8, 16, PanelKind::Custom("Status".into())),
        )
    }

    /// Sets the ratio of the split at `index` (depth-first, split nodes only), clamped to
    /// [`MIN_RATIO`]..[`MAX_RATIO`]. Returns false when no split has that index.
    pub fn set_ratio_at(&mut self, index: usize, ratio: f32) -> bool {
        let mut cursor = 0;
        set_ratio_inner(self, index, ratio.clamp(MIN_RATIO, MAX_RATIO), &mut cursor)
    }

    /// Ratio of the split at `index`, if it exists.
    pub fn ratio_at(&self, index: usize) -> Option<f32> {
        let mut cursor = 0;
        ratio_inner(self, index, &mut cursor)
    }

    /// Every tab in tree order.
    pub fn panels(&self) -> Vec<(PanelId, &PanelKind)> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<(PanelId, &'a PanelKind)>) {
        match self {
            Layout::Panel { tabs, .. } => out.extend(tabs.iter().map(|tab| (tab.id, &tab.kind))),
            Layout::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    /// The tab with `id`, if present.
    pub fn find_tab(&self, id: PanelId) -> Option<&PanelTab> {
        match self {
            Layout::Panel { tabs, .. } => tabs.iter().find(|tab| tab.id == id),
            Layout::Split { first, second, .. } => {
                first.find_tab(id).or_else(|| second.find_tab(id))
            }
        }
    }

    /// Kind of the tab with `id`, if present.
    pub fn find(&self, id: PanelId) -> Option<&PanelKind> {
        self.find_tab(id).map(|tab| &tab.kind)
    }

    /// Tabs of the dock with `id`, if that dock exists.
    pub fn tabs(&self, dock: PanelId) -> Option<&[PanelTab]> {
        let node = self.dock_ref(dock)?;
        match node {
            Layout::Panel { tabs, .. } => Some(tabs),
            Layout::Split { .. } => None,
        }
    }

    /// The visible tab of the dock with `id`, if that dock exists.
    pub fn active_tab(&self, dock: PanelId) -> Option<&PanelTab> {
        let Layout::Panel { tabs, active, .. } = self.dock_ref(dock)? else {
            return None;
        };
        tabs.get(*active)
    }

    /// Number of tabs in the dock with `id`, if that dock exists.
    pub fn tab_count(&self, dock: PanelId) -> Option<usize> {
        Some(self.tabs(dock)?.len())
    }

    /// Moves the divider bounding `id` to `ratio`, clamped to [`MIN_RATIO`]..=[`MAX_RATIO`].
    pub fn set_ratio(&mut self, id: PanelId, ratio: f32) {
        self.set_ratio_inner(id, ratio.clamp(MIN_RATIO, MAX_RATIO));
    }

    fn set_ratio_inner(&mut self, id: PanelId, ratio: f32) -> bool {
        let Layout::Split {
            ratio: slot,
            first,
            second,
            ..
        } = self
        else {
            return false;
        };
        if first.set_ratio_inner(id, ratio) || second.set_ratio_inner(id, ratio) {
            return true;
        }
        if first.contains(id) || second.contains(id) {
            *slot = ratio;
            return true;
        }
        false
    }

    /// Removes `tab` from its dock. An emptied dock is removed and its parent
    /// split collapses onto the surviving sibling.
    ///
    /// Returns `None` when the tab is unknown or is the last tab in the tree.
    pub fn detach(&mut self, tab: PanelId) -> Option<PanelTab> {
        if self.panels().len() <= 1 {
            return None;
        }
        let removed = self.take_tab(tab)?;
        let tree = std::mem::replace(
            self,
            Layout::Panel {
                id: PanelId(0),
                tabs: Vec::new(),
                active: 0,
            },
        );
        *self = tree
            .prune()
            .expect("detach removed a tab while others remain in the tree");
        Some(removed)
    }

    fn take_tab(&mut self, id: PanelId) -> Option<PanelTab> {
        match self {
            Layout::Panel { tabs, active, .. } => {
                let index = tabs.iter().position(|tab| tab.id == id)?;
                let removed = tabs.remove(index);
                let len = tabs.len();
                *active = if index < *active {
                    *active - 1
                } else if index == *active {
                    index.min(len.saturating_sub(1))
                } else {
                    *active
                };
                Some(removed)
            }
            Layout::Split { first, second, .. } => {
                first.take_tab(id).or_else(|| second.take_tab(id))
            }
        }
    }

    fn prune(self) -> Option<Layout> {
        match self {
            Layout::Panel { tabs, .. } if tabs.is_empty() => None,
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => match ((*first).prune(), (*second).prune()) {
                (None, None) => None,
                (Some(first), None) => Some(first),
                (None, Some(second)) => Some(second),
                (Some(first), Some(second)) => Some(Layout::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
            },
            leaf => Some(leaf),
        }
    }

    /// Appends `tab` to the dock with `id` and makes it active.
    ///
    /// Returns `false` for an unknown dock or a tab that is already in the tree.
    pub fn drop_as_tab(&mut self, dock: PanelId, tab: PanelTab) -> bool {
        self.drop_as_tab_at(dock, tab, usize::MAX)
    }

    /// Inserts `tab` into the dock with `id` at `index`, clamped to the current
    /// tab count, and makes it active.
    ///
    /// Returns `false` for an unknown dock or a tab that is already in the tree.
    pub fn drop_as_tab_at(&mut self, dock: PanelId, tab: PanelTab, index: usize) -> bool {
        if self.find_tab(tab.id).is_some() {
            return false;
        }
        let Some(slot) = self.find_dock_mut(dock) else {
            return false;
        };
        let Layout::Panel { tabs, active, .. } = slot else {
            return false;
        };
        let index = index.min(tabs.len());
        tabs.insert(index, tab);
        *active = index;
        true
    }

    /// Replaces the dock with `id` by a split at ratio `0.5`, placing `tab` on
    /// `edge` and keeping the target's remaining tabs in the other child.
    ///
    /// Returns `false` for an unknown dock, an empty tab list, or a tab that is
    /// already in the tree.
    pub fn drop_as_split(&mut self, dock: PanelId, edge: Edge, tab: PanelTab) -> bool {
        if self.find_tab(tab.id).is_some() {
            return false;
        }
        let mut new_dock = self.next_id();
        if new_dock == tab.id {
            new_dock = PanelId(new_dock.0 + 1);
        }
        let Some(slot) = self.find_dock_mut(dock) else {
            return false;
        };
        let Layout::Panel { tabs, .. } = &*slot else {
            return false;
        };
        if tabs.is_empty() {
            return false;
        }

        let Layout::Panel {
            id: leaf_id,
            tabs,
            active,
        } = std::mem::replace(
            slot,
            Layout::Panel {
                id: PanelId(0),
                tabs: Vec::new(),
                active: 0,
            },
        )
        else {
            unreachable!("find_dock_mut only returns panel leaves")
        };

        let moved = Layout::Panel {
            id: new_dock,
            tabs: vec![tab],
            active: 0,
        };
        let rest = Layout::Panel {
            id: leaf_id,
            tabs,
            active,
        };
        let (axis, moved_first) = edge.split();
        *slot = if moved_first {
            Layout::Split {
                axis,
                ratio: 0.5,
                first: Box::new(moved),
                second: Box::new(rest),
            }
        } else {
            Layout::Split {
                axis,
                ratio: 0.5,
                first: Box::new(rest),
                second: Box::new(moved),
            }
        };
        true
    }

    /// Selects the tab at `index` in the dock with `id`.
    ///
    /// Returns `false` for an unknown dock, an out-of-range index, or an index
    /// that is already active.
    pub fn set_active(&mut self, dock: PanelId, index: usize) -> bool {
        let Some(slot) = self.find_dock_mut(dock) else {
            return false;
        };
        let Layout::Panel { tabs, active, .. } = slot else {
            return false;
        };
        if index >= tabs.len() || *active == index {
            return false;
        }
        *active = index;
        true
    }

    /// Checks structural invariants: every dock has tabs, ids are unique, the
    /// active index is in range, and ratios stay within the allowed band.
    pub fn validate(&self) -> Result<(), String> {
        let mut docks = HashSet::new();
        let mut tabs = HashSet::new();
        self.check(&mut docks, &mut tabs)
    }

    fn check(
        &self,
        docks: &mut HashSet<PanelId>,
        tabs: &mut HashSet<PanelId>,
    ) -> Result<(), String> {
        match self {
            Layout::Panel {
                id,
                tabs: list,
                active,
            } => {
                if list.is_empty() {
                    return Err(format!("dock {} has no tabs", id.0));
                }
                if !docks.insert(*id) {
                    return Err(format!("duplicate dock id {}", id.0));
                }
                if *active >= list.len() {
                    return Err(format!(
                        "dock {} active index {active} is out of range",
                        id.0
                    ));
                }
                for tab in list {
                    if !tabs.insert(tab.id) {
                        return Err(format!("duplicate tab id {}", tab.id.0));
                    }
                }
                Ok(())
            }
            Layout::Split {
                ratio,
                first,
                second,
                ..
            } => {
                if !ratio.is_finite() || !(MIN_RATIO..=MAX_RATIO).contains(ratio) {
                    return Err(format!(
                        "split ratio {ratio} is outside {MIN_RATIO}..={MAX_RATIO}"
                    ));
                }
                first.check(docks, tabs)?;
                second.check(docks, tabs)
            }
        }
    }

    /// First unused id, shared by dock and tab identifiers.
    pub fn next_id(&self) -> PanelId {
        PanelId(self.max_id().saturating_add(1))
    }

    fn max_id(&self) -> u32 {
        match self {
            Layout::Panel { id, tabs, .. } => tabs
                .iter()
                .map(|tab| tab.id.0)
                .chain([id.0])
                .max()
                .unwrap_or_default(),
            Layout::Split { first, second, .. } => first.max_id().max(second.max_id()),
        }
    }

    fn contains(&self, id: PanelId) -> bool {
        self.find_tab(id).is_some()
    }

    fn dock_ref(&self, id: PanelId) -> Option<&Layout> {
        match self {
            Layout::Panel { id: pid, .. } if *pid == id => Some(self),
            Layout::Panel { .. } => None,
            Layout::Split { first, second, .. } => {
                first.dock_ref(id).or_else(|| second.dock_ref(id))
            }
        }
    }

    fn find_dock_mut(&mut self, id: PanelId) -> Option<&mut Layout> {
        if let Layout::Split { first, second, .. } = self {
            return first.find_dock_mut(id).or_else(|| second.find_dock_mut(id));
        }
        matches!(self, Layout::Panel { id: pid, .. } if *pid == id).then_some(self)
    }

    #[cfg(test)]
    fn ratio_for(&self, id: PanelId) -> Option<f32> {
        let Layout::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return None;
        };
        if let Some(r) = first.ratio_for(id).or_else(|| second.ratio_for(id)) {
            return Some(r);
        }
        (first.contains(id) || second.contains(id)).then_some(*ratio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(layout: &Layout) -> Layout {
        let text = ron::ser::to_string_pretty(
            layout,
            ron::ser::PrettyConfig::default().struct_names(true),
        )
        .unwrap();
        let parsed: Layout = ron::from_str(&text).unwrap();
        parsed
    }

    fn dock_tabs(layout: &Layout, dock: u32) -> Vec<(u32, PanelKind)> {
        layout
            .tabs(PanelId(dock))
            .unwrap()
            .iter()
            .map(|tab| (tab.id.0, tab.kind.clone()))
            .collect()
    }

    #[test]
    fn ratios_can_be_read_and_set_by_split_index() {
        let mut layout = Layout::default_editor();
        let before = layout.ratio_at(0).unwrap();
        assert!(layout.set_ratio_at(0, before - 0.2));
        assert!((layout.ratio_at(0).unwrap() - (before - 0.2)).abs() < 1e-6);

        assert!(layout.set_ratio_at(1, 0.5));
        assert!((layout.ratio_at(1).unwrap() - 0.5).abs() < 1e-6);
        assert!(
            !layout.set_ratio_at(99, 0.5),
            "an unknown split index must be refused"
        );
        assert_eq!(layout.ratio_at(99), None);
    }

    #[test]
    fn ratios_clamp_to_the_allowed_range() {
        let mut layout = Layout::default_editor();
        layout.set_ratio_at(1, -5.0);
        assert!((layout.ratio_at(1).unwrap() - MIN_RATIO).abs() < 1e-6);
        layout.set_ratio_at(1, 5.0);
        assert!((layout.ratio_at(1).unwrap() - MAX_RATIO).abs() < 1e-6);
    }

    #[test]
    fn default_layout_has_eight_regions() {
        let layout = Layout::default_editor();
        let panels = layout.panels();

        assert_eq!(panels.len(), 8);
        layout.validate().unwrap();
        assert!(panels
            .iter()
            .any(|(_, kind)| **kind == PanelKind::Hierarchy));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Scene));
        assert!(panels
            .iter()
            .any(|(_, kind)| **kind == PanelKind::Inspector));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Project));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Console));
        assert_eq!(layout.next_id(), PanelId(17));
        assert!(layout.find(PanelId(17)).is_none());
        assert!(layout.find(PanelId(11)).is_some());
    }

    #[test]
    fn detach_collapses_split_and_promotes_the_sibling() {
        let mut layout = Layout::default_editor();

        let tab = layout.detach(PanelId(9)).expect("hierarchy tab detaches");
        assert_eq!(tab.kind, PanelKind::Hierarchy);
        assert_eq!(layout.panels().len(), 7);
        assert!(layout.tabs(PanelId(1)).is_none());
        assert_eq!(
            dock_tabs(&layout, 2),
            vec![(10, PanelKind::Custom("Outline".into()))]
        );
        layout.validate().unwrap();
    }

    #[test]
    fn detaching_the_only_tab_of_the_tree_fails() {
        let mut layout = Layout::Panel {
            id: PanelId(1),
            tabs: vec![PanelTab {
                id: PanelId(2),
                kind: PanelKind::Scene,
            }],
            active: 0,
        };

        assert!(layout.detach(PanelId(2)).is_none());
        assert_eq!(layout.panels().len(), 1);
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_tab_appends_and_activates() {
        let mut layout = Layout::default_editor();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(layout.drop_as_tab(PanelId(1), tab));
        assert_eq!(
            dock_tabs(&layout, 1),
            vec![(9, PanelKind::Hierarchy), (14, PanelKind::Inspector),]
        );
        assert_eq!(layout.tabs(PanelId(1)).unwrap().len(), 2);
        assert_eq!(layout.active_tab(PanelId(1)).unwrap().id, PanelId(14));
        layout.validate().unwrap();
    }

    fn stacked_layout() -> Layout {
        let mut layout = Layout::default_editor();
        let console = layout.detach(PanelId(13)).unwrap();
        assert!(layout.drop_as_tab(PanelId(4), console));
        layout
    }

    #[test]
    fn drop_as_tab_at_inserts_at_the_start() {
        let mut layout = stacked_layout();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(layout.drop_as_tab_at(PanelId(4), tab, 0));
        assert_eq!(
            dock_tabs(&layout, 4),
            vec![
                (14, PanelKind::Inspector),
                (12, PanelKind::Project),
                (13, PanelKind::Console),
            ]
        );
        assert_eq!(layout.active_tab(PanelId(4)).unwrap().id, PanelId(14));
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_tab_at_inserts_in_the_middle() {
        let mut layout = stacked_layout();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(layout.drop_as_tab_at(PanelId(4), tab, 1));
        assert_eq!(
            dock_tabs(&layout, 4),
            vec![
                (12, PanelKind::Project),
                (14, PanelKind::Inspector),
                (13, PanelKind::Console),
            ]
        );
        assert_eq!(layout.active_tab(PanelId(4)).unwrap().id, PanelId(14));
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_tab_at_clamps_past_the_end() {
        let mut layout = stacked_layout();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(layout.drop_as_tab_at(PanelId(4), tab, usize::MAX));
        assert_eq!(
            dock_tabs(&layout, 4),
            vec![
                (12, PanelKind::Project),
                (13, PanelKind::Console),
                (14, PanelKind::Inspector),
            ]
        );
        assert_eq!(layout.active_tab(PanelId(4)).unwrap().id, PanelId(14));
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_tab_at_rejects_an_unknown_dock() {
        let mut layout = stacked_layout();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(!layout.drop_as_tab_at(PanelId(99), tab, 0));
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_tab_at_reorders_within_one_dock() {
        let mut layout = stacked_layout();
        let tab = layout.detach(PanelId(12)).unwrap();

        assert!(layout.drop_as_tab_at(PanelId(4), tab, 1));
        assert_eq!(
            dock_tabs(&layout, 4),
            vec![(13, PanelKind::Console), (12, PanelKind::Project)]
        );
        assert_eq!(layout.active_tab(PanelId(4)).unwrap().id, PanelId(12));
        layout.validate().unwrap();
    }

    #[test]
    fn drop_as_split_keeps_target_tabs_and_sets_the_axis() {
        let mut layout = Layout::default_editor();
        let tab = layout.detach(PanelId(14)).unwrap();

        assert!(layout.drop_as_split(PanelId(3), Edge::Right, tab));
        assert_eq!(dock_tabs(&layout, 3), vec![(11, PanelKind::Scene)]);
        let new_id = layout.next_id().0 - 1;
        assert_eq!(dock_tabs(&layout, new_id), vec![(14, PanelKind::Inspector)]);

        let split = split_parent(&layout, PanelId(3)).expect("target dock sits inside a split");
        let Layout::Split {
            axis, ratio, first, ..
        } = split
        else {
            panic!("target dock should sit inside a split");
        };
        assert_eq!(*axis, Axis::Horizontal);
        assert_eq!(*ratio, 0.5);
        assert!(matches!(first.as_ref(), Layout::Panel { id, .. } if *id == PanelId(3)));
        layout.validate().unwrap();
    }

    #[test]
    fn dropping_a_tab_onto_its_own_dock_is_a_no_op() {
        let mut layout = Layout::default_editor();
        let only = layout.find_tab(PanelId(11)).unwrap().clone();

        assert!(!layout.drop_as_tab(PanelId(3), only.clone()));
        assert!(!layout.drop_as_split(PanelId(3), Edge::Left, only.clone()));
        assert_eq!(layout.tabs(PanelId(3)).unwrap().len(), 1);
        layout.validate().unwrap();
    }

    #[test]
    fn a_multi_tab_dock_can_split_itself() {
        let mut layout = Layout::default_editor();
        let tab = layout.detach(PanelId(14)).unwrap();
        layout.drop_as_tab(PanelId(1), tab).then_some(()).unwrap();
        let moved = layout.detach(PanelId(9)).unwrap();

        assert!(layout.drop_as_split(PanelId(1), Edge::Bottom, moved));
        layout.validate().unwrap();
    }

    #[test]
    fn set_active_rejects_unknown_bounds_and_no_ops() {
        let mut layout = Layout::default_editor();
        let tab = layout.detach(PanelId(14)).unwrap();
        layout.drop_as_tab(PanelId(1), tab);

        assert!(layout.set_active(PanelId(1), 0));
        assert!(!layout.set_active(PanelId(1), 0));
        assert!(!layout.set_active(PanelId(1), 9));
        assert!(!layout.set_active(PanelId(99), 0));
        layout.validate().unwrap();
    }

    #[test]
    fn validate_holds_after_a_chain_of_moves() {
        let mut layout = Layout::default_editor();
        layout.validate().unwrap();

        for (from, dock, edge) in [
            (14, 1, None),
            (9, 4, None),
            (16, 3, Some(Edge::Bottom)),
            (12, 2, Some(Edge::Left)),
        ] {
            let tab = layout.detach(PanelId(from)).unwrap();
            let ok = match edge {
                Some(edge) => layout.drop_as_split(PanelId(dock), edge, tab),
                None => layout.drop_as_tab(PanelId(dock), tab),
            };
            assert!(ok, "move of {from} onto {dock} failed");
            layout.validate().unwrap();
        }
    }

    #[test]
    fn ron_round_trips_a_two_tab_panel() {
        let mut layout = Layout::default_editor();
        let tab = layout.detach(PanelId(14)).unwrap();
        assert!(layout.drop_as_tab(PanelId(1), tab));

        let parsed = round_trip(&layout);
        assert_eq!(parsed, layout);
        parsed.validate().unwrap();
        assert_eq!(parsed.tabs(PanelId(1)).unwrap().len(), 2);
    }

    #[test]
    fn ratio_is_clamped() {
        let mut layout = Layout::default_editor();

        layout.set_ratio(PanelId(9), 0.0);
        assert_eq!(layout.ratio_for(PanelId(9)), Some(MIN_RATIO));

        layout.set_ratio(PanelId(9), 1.5);
        assert_eq!(layout.ratio_for(PanelId(9)), Some(MAX_RATIO));

        layout.set_ratio(PanelId(9), 0.42);
        assert_eq!(layout.ratio_for(PanelId(9)), Some(0.42));

        layout.set_ratio(PanelId(11), 0.3);
        assert_eq!(layout.ratio_for(PanelId(11)), Some(0.3));
    }

    fn split_parent<'a>(layout: &'a Layout, dock: PanelId) -> Option<&'a Layout> {
        let Layout::Split { first, second, .. } = layout else {
            return None;
        };
        if matches!(first.as_ref(), Layout::Panel { id, .. } if *id == dock)
            || matches!(second.as_ref(), Layout::Panel { id, .. } if *id == dock)
        {
            return Some(layout);
        }
        split_parent(first, dock).or_else(|| split_parent(second, dock))
    }
}
