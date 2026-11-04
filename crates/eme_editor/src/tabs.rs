//! Open scene tabs: one document per tab, cached in memory while the editor runs.
//!
//! Switching stashes the live document before replacing it, so unsaved edits
//! survive a round trip through another tab.

use eme::feathers::theme::UiTheme;
use eme::prelude::*;
use eme::text::TextColor;
use eme::ui::BackgroundColor;
use eme_game_scene::SceneDoc;

use crate::persist;
use crate::scene::SceneState;
use crate::tokens;
use crate::undo::Undo;

/// Height of the accent underline under the active scene tab.
const TAB_UNDERLINE: f32 = 2.0;

/// One scene tab and the file it saves to.
pub struct Tab {
    pub label: &'static str,
    pub path: &'static str,
}

/// The scene tabs, in strip order.
pub const TABS: [Tab; 2] = [
    Tab {
        label: "main.scene",
        path: "scenes/main.ember",
    },
    Tab {
        label: "player.scene",
        path: "scenes/player.ember",
    },
];

/// Marks a scene tab button with its index in [`TABS`].
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SceneTab(pub usize);

/// Marks the label inside a scene tab with its index in [`TABS`].
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SceneTabLabel(pub usize);

/// The open scene tabs, which one is showing, and the documents kept for the rest.
#[derive(Resource)]
pub struct OpenScenes {
    active: usize,
    cache: [Option<SceneDoc>; TABS.len()],
}

impl Default for OpenScenes {
    fn default() -> Self {
        Self {
            active: 0,
            cache: core::array::from_fn(|_| None),
        }
    }
}

impl OpenScenes {
    /// The path Ctrl+S writes for the tab that is currently showing.
    pub fn path(&self) -> &'static str {
        TABS[self.active].path
    }

    /// Shows tab `index`, stashing the document being left behind.
    ///
    /// The target document comes from the tab's cache when it has one, otherwise
    /// from its file, and an unreadable or missing file leaves an empty document.
    /// The undo history starts over because it belongs to the tab being entered.
    pub fn switch(&mut self, index: usize, state: &mut SceneState, undo: &mut Undo) {
        self.switch_using(index, state, undo, |path| match persist::load_from(path) {
            Ok(doc) => doc,
            Err(err) => {
                error!("could not load {path}: {err}");
                None
            }
        });
    }

    fn switch_using<F>(&mut self, index: usize, state: &mut SceneState, undo: &mut Undo, load: F)
    where
        F: FnOnce(&str) -> Option<SceneDoc>,
    {
        if index >= TABS.len() || index == self.active {
            return;
        }
        self.cache[self.active] = Some(state.doc.clone());
        self.active = index;
        let doc = match self.cache[index].clone() {
            Some(doc) => doc,
            None => load(TABS[index].path).unwrap_or_default(),
        };
        state.replace_doc(doc);
        undo.clear();
    }
}

/// Switches to the scene tab that was clicked.
pub fn tab_click(
    click: On<PointerClick>,
    tabs: Query<&SceneTab>,
    parents: Query<&ChildOf>,
    mut scenes: ResMut<OpenScenes>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(index) = nearest(click.entity, &tabs, &parents) else {
        return;
    };
    scenes.switch(index.0, &mut state, &mut undo);
}

/// Paints the active tab with the panel fill, underline, and bright text.
pub fn sync(
    scenes: Res<OpenScenes>,
    theme: Res<UiTheme>,
    mut tabs: Query<(&SceneTab, &mut BackgroundColor, &mut Node)>,
    mut labels: Query<(&SceneTabLabel, &mut TextColor)>,
) {
    let active_bg = theme.color(&tokens::PANEL);
    let idle_bg = theme.color(&tokens::WINDOW);
    let active_fg = theme.color(&tokens::TEXT);
    let idle_fg = theme.color(&tokens::TEXT_DIM);

    for (tab, mut bg, mut node) in &mut tabs {
        let on = tab.0 == scenes.active;
        let want = if on { active_bg } else { idle_bg };
        if bg.0 != want {
            bg.0 = want;
        }
        let line = px(if on { TAB_UNDERLINE } else { 0.0 });
        if node.border.bottom != line {
            node.border.bottom = line;
        }
    }

    for (tab, mut fg) in &mut labels {
        let want = if tab.0 == scenes.active {
            active_fg
        } else {
            idle_fg
        };
        if fg.0 != want {
            fg.0 = want;
        }
    }
}

fn nearest<T: Component + Copy>(
    mut entity: Entity,
    markers: &Query<&T>,
    parents: &Query<&ChildOf>,
) -> Option<T> {
    loop {
        if let Ok(marker) = markers.get(entity) {
            return Some(*marker);
        }
        let parent = parents.get(entity).ok()?;
        entity = parent.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    #[test]
    fn switching_stashes_the_document_being_left() {
        let mut state = SceneState::default();
        let main_id = state.spawn(ObjectKind::Cube);
        let mut undo = Undo::default();
        let mut scenes = OpenScenes::default();

        scenes.switch_using(1, &mut state, &mut undo, |_| Some(SceneDoc::new()));
        assert!(state.doc.is_empty());

        scenes.switch_using(0, &mut state, &mut undo, |_| None);
        assert_eq!(state.doc.len(), 1);
        assert!(state.doc.get(main_id).is_some());
    }

    #[test]
    fn switching_back_keeps_unsaved_edits() {
        let mut state = SceneState::default();
        state.spawn(ObjectKind::Cube);
        let mut undo = Undo::default();
        let mut scenes = OpenScenes::default();

        scenes.switch_using(1, &mut state, &mut undo, |_| Some(SceneDoc::new()));
        let player_id = state.spawn(ObjectKind::Sphere);

        scenes.switch_using(0, &mut state, &mut undo, |_| None);
        scenes.switch_using(1, &mut state, &mut undo, |_| None);

        assert_eq!(state.doc.len(), 1);
        assert!(state.doc.get(player_id).is_some());
    }

    #[test]
    fn uncached_tab_without_a_file_is_empty_and_drops_undo() {
        let mut state = SceneState::default();
        state.spawn(ObjectKind::Cube);
        let mut undo = Undo::default();
        undo.checkpoint(&state, "Spawn");
        assert!(undo.can_undo());

        let mut scenes = OpenScenes::default();
        scenes.switch_using(1, &mut state, &mut undo, |_| None);

        assert!(state.doc.is_empty());
        assert!(!undo.can_undo());
        assert!(!undo.can_redo());
    }
}
