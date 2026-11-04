//! Undo and redo: whole-document snapshots, because every edit funnels through
//! [`SceneState`] and a scene is small enough to copy.

use eme::prelude::*;
use eme_editor_core::Selection;
use eme_game_scene::SceneDoc;

use crate::scene::SceneState;

/// How many edits the history keeps before dropping the oldest.
const LIMIT: usize = 200;

#[derive(Clone)]
struct Snapshot {
    label: &'static str,
    doc: SceneDoc,
    selection: Selection<u64>,
    anchor: Option<u64>,
}

/// Undo history for the scene document.
#[derive(Resource, Default)]
pub struct Undo {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    epoch: u64,
}

impl Undo {
    /// Records the document as it is now, before an edit called `label` runs.
    ///
    /// An entry is skipped when the previous one already holds the same document,
    /// so repeated shortcuts without an edit in between cannot flood the history.
    pub fn checkpoint(&mut self, state: &SceneState, label: &'static str) {
        self.sync(state.epoch());
        if self.past.last().is_some_and(|snap| snap.doc == state.doc) {
            return;
        }
        if self.past.len() == LIMIT {
            self.past.remove(0);
        }
        let snap = self.snapshot(state, label);
        self.past.push(snap);
        self.future.clear();
    }

    /// Restores the document, selection, and anchor from before the last edit.
    pub fn undo(&mut self, state: &mut SceneState) {
        self.sync(state.epoch());
        let Some(prev) = self.past.pop() else {
            return;
        };
        let snap = self.snapshot(state, prev.label);
        self.future.push(snap);
        self.restore(state, prev);
    }

    /// Re-applies the edit the last [`Self::undo`] reverted.
    pub fn redo(&mut self, state: &mut SceneState) {
        self.sync(state.epoch());
        let Some(next) = self.future.pop() else {
            return;
        };
        let snap = self.snapshot(state, next.label);
        self.past.push(snap);
        self.restore(state, next);
    }

    /// Whether there is an edit to undo.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Labels of the recorded edits, most recent first; the first one is the
    /// state the document is in now.
    pub fn names(&self) -> impl Iterator<Item = &'static str> {
        self.past.iter().rev().map(|snap| snap.label)
    }

    /// Whether there is an undone edit to re-apply.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Drops the whole history, as opening another scene does.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    fn snapshot(&self, state: &SceneState, label: &'static str) -> Snapshot {
        Snapshot {
            label,
            doc: state.doc.clone(),
            selection: state.selection.clone(),
            anchor: state.anchor,
        }
    }

    fn restore(&mut self, state: &mut SceneState, snap: Snapshot) {
        state.restore(snap.doc, snap.selection, snap.anchor);
    }

    fn sync(&mut self, epoch: u64) {
        if self.epoch == epoch {
            return;
        }
        self.epoch = epoch;
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::{ObjectKind, SceneObject};

    fn two_cubes() -> (SceneState, u64, u64) {
        let mut state = SceneState::default();
        let a = state.spawn(ObjectKind::Cube);
        let b = state.spawn(ObjectKind::Cube);
        (state, a, b)
    }

    #[test]
    fn undo_restores_doc_and_selection() {
        let (mut state, a, _) = two_cubes();
        let mut undo = Undo::default();

        state.select_only(a);
        undo.checkpoint(&state, "Spawn");
        let c = state.spawn(ObjectKind::Sphere);
        state.select_only(c);
        assert_eq!(state.doc.len(), 3);

        undo.undo(&mut state);
        assert_eq!(state.doc.len(), 2);
        assert_eq!(state.primary(), Some(a));
        assert!(state.doc.get(c).is_none());

        undo.redo(&mut state);
        assert_eq!(state.doc.len(), 3);
        assert_eq!(state.primary(), Some(c));
        assert_eq!(state.doc.get(c).unwrap().name, "Sphere 1");
    }

    #[test]
    fn undo_restores_the_anchor() {
        let (mut state, a, b) = two_cubes();
        let mut undo = Undo::default();
        state.select_only(a);
        state.extend_to(b);
        undo.checkpoint(&state, "Delete");
        state.despawn_selected();

        undo.undo(&mut state);
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![a, b]);
        assert_eq!(state.anchor, Some(a));
    }

    #[test]
    fn duplicate_checkpoints_are_dropped() {
        let (state, ..) = two_cubes();
        let mut undo = Undo::default();
        undo.checkpoint(&state, "Move");
        undo.checkpoint(&state, "Move");
        assert_eq!(undo.past.len(), 1);
        assert!(undo.can_undo());
    }

    #[test]
    fn names_run_newest_first() {
        let (mut state, a, _) = two_cubes();
        let mut undo = Undo::default();
        undo.checkpoint(&state, "Spawn");
        state.despawn(a);
        undo.checkpoint(&state, "Delete");
        state.spawn(ObjectKind::Sphere);
        assert_eq!(undo.names().collect::<Vec<_>>(), vec!["Delete", "Spawn"]);
    }

    #[test]
    fn checkpoint_clears_redo() {
        let (mut state, a, _) = two_cubes();
        let mut undo = Undo::default();
        undo.checkpoint(&state, "Delete");
        state.despawn(a);
        undo.undo(&mut state);
        assert!(undo.can_redo());

        undo.checkpoint(&state, "Spawn");
        assert!(!undo.can_redo());
    }

    #[test]
    fn empty_history_ignores_undo_and_redo() {
        let (mut state, ..) = two_cubes();
        let mut undo = Undo::default();
        undo.undo(&mut state);
        undo.redo(&mut state);
        assert_eq!(state.doc.len(), 2);
    }

    #[test]
    fn replacing_the_doc_drops_the_history() {
        let (mut state, a, _) = two_cubes();
        let mut undo = Undo::default();
        undo.checkpoint(&state, "Delete");
        state.despawn(a);
        assert!(undo.can_undo());

        state.replace_doc(SceneDoc::new());
        undo.undo(&mut state);
        assert!(state.doc.is_empty());
        assert!(!undo.can_undo());
    }

    #[test]
    fn history_drops_the_oldest_entry_past_the_limit() {
        let (mut state, ..) = two_cubes();
        let mut undo = Undo::default();
        for i in 0..LIMIT + 5 {
            let obj = SceneObject {
                id: 100 + i as u64,
                ..state.doc.objects[0].clone()
            };
            undo.checkpoint(&state, "Move");
            state.doc.objects.push(obj);
        }
        assert_eq!(undo.past.len(), LIMIT);
    }
}
