//! Copy, cut, paste, and duplicate over the scene document.
//!
//! Pasted objects always get a fresh id and a freshly allocated name, so the
//! document keeps its id and name invariants no matter how often a copy lands.

use eme::prelude::*;
use eme_game_scene::SceneObject;

use crate::scene::SceneState;
use crate::undo::Undo;

/// Objects copied from the scene, in authoring order.
#[derive(Resource, Default)]
pub struct Clipboard(pub Vec<SceneObject>);

/// Copies every selected object into `clip`; returns how many were copied.
pub fn copy_selection(state: &SceneState, clip: &mut Clipboard) -> usize {
    clip.0 = state
        .doc
        .iter()
        .filter(|obj| state.is_selected(obj.id))
        .cloned()
        .collect();
    clip.0.len()
}

/// Copies the selection and then deletes it, as one undo step.
pub fn cut_selection(state: &mut SceneState, clip: &mut Clipboard, undo: &mut Undo) -> usize {
    let copied = copy_selection(state, clip);
    if copied == 0 {
        return 0;
    }
    undo.checkpoint(state, "Cut");
    state.despawn_selected();
    copied
}

/// Pastes the clipboard one unit along X and Z; the copies become the selection.
pub fn paste(state: &mut SceneState, clip: &Clipboard, undo: &mut Undo) -> usize {
    insert(state, clip, [1.0, 0.0, 1.0], undo, "Paste")
}

/// Copies the selection and pastes the copies one unit along X, in place.
pub fn duplicate_selection(state: &mut SceneState, undo: &mut Undo) -> usize {
    let mut clip = Clipboard::default();
    if copy_selection(state, &mut clip) == 0 {
        return 0;
    }
    insert(state, &clip, [1.0, 0.0, 0.0], undo, "Duplicate")
}

fn insert(
    state: &mut SceneState,
    clip: &Clipboard,
    offset: [f32; 3],
    undo: &mut Undo,
    label: &'static str,
) -> usize {
    if clip.0.is_empty() {
        return 0;
    }
    undo.checkpoint(state, label);
    let mut ids = Vec::with_capacity(clip.0.len());
    for src in &clip.0 {
        let mut transform = src.transform();
        transform.translation[0] += offset[0];
        transform.translation[2] += offset[2];
        let name = state.next_name(src.kind);
        let id = state.doc.add(src.kind, name, transform);
        if let Some(obj) = state.doc.get_mut(id) {
            obj.color = src.color;
        }
        ids.push(id);
    }
    state.select_many(&ids);
    state.touch();
    ids.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    fn state_with_two() -> (SceneState, u64, u64) {
        let mut state = SceneState::default();
        let a = state.spawn(ObjectKind::Cube);
        let b = state.spawn(ObjectKind::Sphere);
        (state, a, b)
    }

    #[test]
    fn copy_takes_every_selected_object() {
        let (mut state, a, b) = state_with_two();
        state.select_many(&[a, b]);
        let mut clip = Clipboard::default();
        assert_eq!(copy_selection(&state, &mut clip), 2);
        assert_eq!(clip.0[0].id, a);
        assert_eq!(clip.0[1].id, b);
    }

    #[test]
    fn paste_assigns_fresh_unique_ids_and_names() {
        let (mut state, a, _) = state_with_two();
        state.select_only(a);
        let mut clip = Clipboard::default();
        let mut undo = Undo::default();
        copy_selection(&state, &mut clip);

        assert_eq!(paste(&mut state, &clip, &mut undo), 1);
        assert_eq!(state.doc.len(), 3);
        let first = state.primary().unwrap();
        assert_ne!(first, a);
        assert_eq!(state.doc.get(first).unwrap().name, "Cube 2");
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![first]);

        assert_eq!(paste(&mut state, &clip, &mut undo), 1);
        let second = state.primary().unwrap();
        assert_ne!(second, first);
        assert_eq!(state.doc.get(second).unwrap().name, "Cube 3");

        let ids: Vec<u64> = state.doc.iter().map(|obj| obj.id).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn paste_steps_one_cell_on_x_and_z() {
        let (mut state, a, _) = state_with_two();
        state.select_only(a);
        let mut clip = Clipboard::default();
        let mut undo = Undo::default();
        copy_selection(&state, &mut clip);
        let src = state.doc.get(a).unwrap().translation;

        paste(&mut state, &clip, &mut undo);
        let copy = state.primary().unwrap();
        let got = state.doc.get(copy).unwrap().translation;
        assert_eq!(got[0], src[0] + 1.0);
        assert_eq!(got[1], src[1]);
        assert_eq!(got[2], src[2] + 1.0);
    }

    #[test]
    fn cut_removes_the_selection_in_one_step() {
        let (mut state, a, b) = state_with_two();
        state.select_many(&[a, b]);
        let mut clip = Clipboard::default();
        let mut undo = Undo::default();
        assert_eq!(cut_selection(&mut state, &mut clip, &mut undo), 2);
        assert!(state.doc.is_empty());
        assert!(state.selection.is_empty());

        undo.undo(&mut state);
        assert_eq!(state.doc.len(), 2);
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![a, b]);

        paste(&mut state, &clip, &mut undo);
        assert_eq!(state.doc.len(), 4);
    }

    #[test]
    fn duplicate_offsets_x_only_and_selects_the_copies() {
        let (mut state, a, _) = state_with_two();
        state.select_only(a);
        let mut undo = Undo::default();
        let src = state.doc.get(a).unwrap().translation;
        assert_eq!(duplicate_selection(&mut state, &mut undo), 1);

        let copy = state.primary().unwrap();
        assert_ne!(copy, a);
        let got = state.doc.get(copy).unwrap().translation;
        assert_eq!(got[0], src[0] + 1.0);
        assert_eq!(got[2], src[2]);
        assert_eq!(state.doc.get(copy).unwrap().name, "Cube 2");
    }

    #[test]
    fn empty_selection_does_not_touch_the_history() {
        let (mut state, ..) = state_with_two();
        let mut clip = Clipboard::default();
        let mut undo = Undo::default();
        assert_eq!(copy_selection(&state, &mut clip), 0);
        assert_eq!(cut_selection(&mut state, &mut clip, &mut undo), 0);
        assert_eq!(paste(&mut state, &clip, &mut undo), 0);
        assert_eq!(duplicate_selection(&mut state, &mut undo), 0);
        assert!(!undo.can_undo());
        assert_eq!(state.doc.len(), 2);
    }
}
