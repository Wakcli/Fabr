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

/// Stable identifier for a panel.
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

/// Binary tree of splits and panels.
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
    /// A leaf panel.
    Panel {
        /// Identifier of the panel.
        id: PanelId,
        /// Panel contents.
        kind: PanelKind,
    },
}

impl Layout {
    /// The eight-region arrangement used when no layout is saved.
    pub fn default_editor() -> Self {
        let panel = |id: u32, kind: PanelKind| Layout::Panel {
            id: PanelId(id),
            kind,
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
            panel(1, PanelKind::Hierarchy),
            panel(2, PanelKind::Custom("Outline".into())),
        );
        let center = split(
            Axis::Vertical,
            0.70,
            panel(3, PanelKind::Scene),
            split(
                Axis::Horizontal,
                0.5,
                panel(4, PanelKind::Project),
                panel(5, PanelKind::Console),
            ),
        );
        let right = split(
            Axis::Vertical,
            0.62,
            panel(6, PanelKind::Inspector),
            panel(7, PanelKind::Custom("Stats".into())),
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
            panel(8, PanelKind::Custom("Status".into())),
        )
    }

    /// Panels in tree order.
    pub fn panels(&self) -> Vec<(PanelId, &PanelKind)> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<(PanelId, &'a PanelKind)>) {
        match self {
            Layout::Panel { id, kind } => out.push((*id, kind)),
            Layout::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    /// Kind of the panel with `id`, if present.
    pub fn find(&self, id: PanelId) -> Option<&PanelKind> {
        match self {
            Layout::Panel { id: pid, kind } if *pid == id => Some(kind),
            Layout::Panel { .. } => None,
            Layout::Split { first, second, .. } => first.find(id).or_else(|| second.find(id)),
        }
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

    /// Replaces the panel `id` with a split holding the old panel and a new one.
    pub fn split(&mut self, id: PanelId, axis: Axis, kind: PanelKind) -> PanelId {
        let new_id = self.next_id();
        let Some(slot) = self.find_mut(id) else {
            return id;
        };
        let old = std::mem::replace(
            slot,
            Layout::Panel {
                id: new_id,
                kind: kind.clone(),
            },
        );
        *slot = Layout::Split {
            axis,
            ratio: 0.5,
            first: Box::new(old),
            second: Box::new(Layout::Panel { id: new_id, kind }),
        };
        new_id
    }

    /// Removes a panel and collapses its parent split. Removing the last panel fails.
    pub fn remove(&mut self, id: PanelId) -> bool {
        if self.panels().len() < 2 || !self.contains(id) {
            return false;
        }
        let tree = std::mem::replace(self, Self::empty_panel());
        *self = tree.without(id);
        true
    }

    /// First unused panel id.
    pub fn next_id(&self) -> PanelId {
        let max = self
            .panels()
            .iter()
            .map(|(id, _)| id.0)
            .max()
            .unwrap_or_default();
        PanelId(max + 1)
    }

    fn contains(&self, id: PanelId) -> bool {
        match self {
            Layout::Panel { id: pid, .. } => *pid == id,
            Layout::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    fn find_mut(&mut self, id: PanelId) -> Option<&mut Layout> {
        if let Layout::Split { first, second, .. } = self {
            return first.find_mut(id).or_else(|| second.find_mut(id));
        }
        matches!(self, Layout::Panel { id: pid, .. } if *pid == id).then_some(self)
    }

    fn without(self, id: PanelId) -> Layout {
        match self {
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let first_hit =
                    matches!(first.as_ref(), Layout::Panel { id: pid, .. } if *pid == id);
                if first_hit {
                    return *second;
                }
                let second_hit =
                    matches!(second.as_ref(), Layout::Panel { id: pid, .. } if *pid == id);
                if second_hit {
                    return *first;
                }
                Layout::Split {
                    axis,
                    ratio,
                    first: Box::new(first.without(id)),
                    second: Box::new(second.without(id)),
                }
            }
            panel => panel,
        }
    }

    fn empty_panel() -> Self {
        Layout::Panel {
            id: PanelId(0),
            kind: PanelKind::Custom(String::new()),
        }
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

    #[test]
    fn default_layout_has_eight_regions() {
        let layout = Layout::default_editor();
        let panels = layout.panels();

        assert_eq!(panels.len(), 8);
        assert!(panels
            .iter()
            .any(|(_, kind)| **kind == PanelKind::Hierarchy));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Scene));
        assert!(panels
            .iter()
            .any(|(_, kind)| **kind == PanelKind::Inspector));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Project));
        assert!(panels.iter().any(|(_, kind)| **kind == PanelKind::Console));
        assert_eq!(layout.next_id(), PanelId(9));
        assert!(layout.find(PanelId(9)).is_none());
        assert!(layout.find(PanelId(3)).is_some());
    }

    #[test]
    fn split_adds_panel_and_keeps_original() {
        let mut layout = Layout::default_editor();
        let new_id = layout.split(PanelId(3), Axis::Vertical, PanelKind::Custom("Log".into()));

        assert_eq!(new_id, PanelId(9));
        assert_eq!(layout.panels().len(), 9);
        assert_eq!(layout.find(new_id), Some(&PanelKind::Custom("Log".into())));
        assert_eq!(layout.find(PanelId(3)), Some(&PanelKind::Scene));
        assert_eq!(layout.next_id(), PanelId(10));
    }

    #[test]
    fn remove_collapses_split_and_restores_tree() {
        let original = Layout::default_editor();
        let mut layout = original.clone();
        let new_id = layout.split(PanelId(3), Axis::Vertical, PanelKind::Custom("Log".into()));

        assert!(layout.remove(new_id));
        assert_eq!(layout, original);
        assert_eq!(layout.panels().len(), 8);
    }

    #[test]
    fn removing_the_last_panel_fails() {
        let mut layout = Layout::Panel {
            id: PanelId(7),
            kind: PanelKind::Scene,
        };
        assert!(!layout.remove(PanelId(7)));
        assert!(!layout.remove(PanelId(99)));
    }

    #[test]
    fn ratio_is_clamped() {
        let mut layout = Layout::default_editor();

        layout.set_ratio(PanelId(1), 0.0);
        assert_eq!(layout.ratio_for(PanelId(1)), Some(MIN_RATIO));

        layout.set_ratio(PanelId(1), 1.5);
        assert_eq!(layout.ratio_for(PanelId(1)), Some(MAX_RATIO));

        layout.set_ratio(PanelId(1), 0.42);
        assert_eq!(layout.ratio_for(PanelId(1)), Some(0.42));

        layout.set_ratio(PanelId(3), 0.3);
        assert_eq!(layout.ratio_for(PanelId(3)), Some(0.3));
    }

    #[test]
    fn split_survives_ratio_change_and_find() {
        let mut layout = Layout::default_editor();
        layout.set_ratio(PanelId(8), 0.9);
        assert_eq!(layout.ratio_for(PanelId(8)), Some(0.9));
        assert!(layout.find(PanelId(8)).is_some());
    }
}
