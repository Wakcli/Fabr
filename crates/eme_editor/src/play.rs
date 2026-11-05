//! Play modes: run the game in the editor viewport, or launch the shipped
//! runtime in its own window.
//!
//! Embedded play is the default. The editor keeps the authored document as a
//! baseline, swaps the viewport into Game view, and restores the baseline when
//! play stops, so anything edited while playing is thrown away. There is no
//! scripting layer yet, so nothing advances on its own; the point is the mode,
//! the frozen baseline, and the editor overlays getting out of the way.
//!
//! The windowed path writes the document to a scratch file and starts
//! `eme_player --scene <file>` as a child process, which is exactly the runtime
//! a shipped build runs.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use eme::math::Rect;
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button};
use eme::window::{PrimaryWindow, Window};
use eme_editor_core::Selection;
use eme_game_scene::SceneDoc;

use crate::build;
use crate::orbit::ViewMode;
use crate::scene::SceneState;
use crate::screens::editor::chrome::{self, Part, FONT_SIZE_OUTPUT};
use crate::screens::editor::icons;
use crate::tokens;

/// Snapshot the windowed player reads, relative to the cache root.
const PLAY_SCENE: &str = "play/scene.ember";

/// Marks the icon slot that shows while a game is running.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayGlyph(pub bool);

/// Marks the top bar play control so the flyout can follow its rect.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayButton;

/// Marks the root of the hover flyout under the play control.
#[derive(Component, Clone, Copy, Default)]
pub struct PlayFlyout;

/// The document and selection as they were when embedded play started.
struct Snapshot {
    doc: SceneDoc,
    selection: Selection<u64>,
    anchor: Option<u64>,
}

/// The running game, embedded or in its own window.
#[derive(Resource, Default)]
pub struct PlayState {
    child: Option<Child>,
    embedded: Option<Snapshot>,
}

impl PlayState {
    /// Whether a game is running, either embedded or windowed.
    pub fn running(&self) -> bool {
        self.child.is_some() || self.embedded.is_some()
    }

    /// Kills the windowed game process if one is running.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Freezes `state` as the play baseline and switches the viewport to Game.
    pub fn start_embedded(&mut self, state: &mut SceneState, view: &mut ViewMode) {
        if self.embedded.is_some() {
            return;
        }
        self.embedded = Some(Snapshot {
            doc: state.doc.clone(),
            selection: state.selection.clone(),
            anchor: state.anchor,
        });
        *view = ViewMode::Game;
    }

    /// Restores the baseline captured by [`Self::start_embedded`].
    pub fn stop_embedded(&mut self, state: &mut SceneState, view: &mut ViewMode) {
        let Some(snap) = self.embedded.take() else {
            return;
        };
        state.replace_doc(snap.doc);
        state.selection = snap.selection;
        state.anchor = snap.anchor;
        state.touch();
        *view = ViewMode::Editor;
    }

    /// Starts the shipped runtime against `state`'s document in its own window.
    pub fn play_in_window(&mut self, state: &SceneState) {
        if self.child.is_some() {
            return;
        }
        match launch(state) {
            Ok(child) => {
                info!("playing in pid {}", child.id());
                self.child = Some(child);
            }
            Err(err) => error!("could not start the game: {err}"),
        }
    }
}

impl Drop for PlayState {
    fn drop(&mut self) {
        self.stop();
    }
}

/// What the play control does in the current state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Toggle {
    StartEmbedded,
    StopEmbedded,
    StopWindowed,
}

/// One play mode at a time: a windowed game is stopped first, then an embedded
/// session, and only an idle editor starts embedded play.
fn next(child: bool, embedded: bool) -> Toggle {
    if child {
        Toggle::StopWindowed
    } else if embedded {
        Toggle::StopEmbedded
    } else {
        Toggle::StartEmbedded
    }
}

/// Starts embedded play, or stops whichever mode is running.
pub fn toggle(
    _: On<Activate>,
    mut play: ResMut<PlayState>,
    mut scene: ResMut<SceneState>,
    mut view: ResMut<ViewMode>,
    mut hover: ResMut<PlayHover>,
) {
    hover.dismissed = true;
    match next(play.child.is_some(), play.embedded.is_some()) {
        Toggle::StopWindowed => play.stop(),
        Toggle::StopEmbedded => play.stop_embedded(&mut scene, &mut view),
        Toggle::StartEmbedded => play.start_embedded(&mut scene, &mut view),
    }
}

/// Launches the windowed game from the flyout row.
fn play_in_window_row(
    _: On<Activate>,
    mut play: ResMut<PlayState>,
    mut scene: ResMut<SceneState>,
    mut view: ResMut<ViewMode>,
    mut hover: ResMut<PlayHover>,
) {
    play.stop_embedded(&mut scene, &mut view);
    play.play_in_window(&scene);
    hover.dismissed = true;
}

fn launch(state: &SceneState) -> Result<Child, String> {
    let player = build::player_binary()
        .ok_or_else(|| "no eme_player binary; run Build once to compile it".to_string())?;
    let scene = snapshot_path().ok_or_else(|| "no cache directory for the snapshot".to_string())?;
    if let Some(dir) = scene.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let text = state
        .doc
        .to_ron()
        .map_err(|err| format!("could not encode the scene: {err}"))?;
    std::fs::write(&scene, text).map_err(|err| format!("{}: {err}", scene.display()))?;
    player_command(&player, &scene, &build::workspace_dir())
        .spawn()
        .map_err(|err| format!("{}: {err}", player.display()))
}

fn player_command(player: &Path, scene: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(player);
    command.arg("--scene").arg(scene).current_dir(cwd);
    command
}

fn snapshot_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("emberrly").join(PLAY_SCENE))
}

/// Drops the handle once the windowed player exits on its own.
pub fn poll(mut play: ResMut<PlayState>) {
    let finished = match play.child.as_mut() {
        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
        None => false,
    };
    if finished {
        play.child = None;
    }
}

/// Shows the stop glyph while a game runs, the play glyph otherwise.
pub fn sync_button(play: Res<PlayState>, mut glyphs: Query<(&PlayGlyph, &mut Node)>) {
    for (glyph, mut node) in &mut glyphs {
        let want = if glyph.0 {
            play.running()
        } else {
            !play.running()
        };
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Pointer presence over the play control and its flyout, kept as two booleans
/// so travelling between the two does not hide the flyout.
#[derive(Resource, Default)]
pub struct PlayHover {
    button: bool,
    flyout: bool,
    dismissed: bool,
}

impl PlayHover {
    /// Whether the pointer is over the button or the flyout.
    pub fn visible(&self) -> bool {
        self.button || self.flyout
    }

    /// Whether the flyout should be on screen: hovered, and not dismissed by
    /// an activation until the pointer leaves both parts.
    fn shown(&self) -> bool {
        self.visible() && !self.dismissed
    }
}

fn over_button(_: On<PointerOver>, mut hover: ResMut<PlayHover>) {
    hover.button = true;
}

fn out_button(_: On<PointerOut>, mut hover: ResMut<PlayHover>) {
    hover.button = false;
}

fn over_flyout(_: On<PointerOver>, mut hover: ResMut<PlayHover>) {
    hover.flyout = true;
}

fn out_flyout(_: On<PointerOut>, mut hover: ResMut<PlayHover>) {
    hover.flyout = false;
}

/// Shows the flyout whenever the button or the flyout itself is hovered.
pub fn sync_flyout(mut hover: ResMut<PlayHover>, mut roots: Query<&mut Node, With<PlayFlyout>>) {
    if hover.dismissed && !hover.visible() {
        hover.dismissed = false;
    }
    let want = if hover.shown() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Keeps the flyout flush under the play control's current screen rect.
pub fn place_flyout(
    window: Single<&Window, With<PrimaryWindow>>,
    buttons: Query<(&ComputedNode, &UiGlobalTransform), With<PlayButton>>,
    mut roots: Query<&mut Node, With<PlayFlyout>>,
) {
    let Ok((node, global)) = buttons.single() else {
        return;
    };
    let scale = window.scale_factor();
    let rect = Rect::from_center_size(global.translation.trunc(), node.size());
    let left = px(rect.min.x / scale);
    let top = px(rect.max.y / scale);
    for mut flyout in &mut roots {
        if flyout.left != left || flyout.top != top {
            flyout.left = left;
            flyout.top = top;
        }
    }
}

/// The top bar play/stop control.
pub fn button() -> Part {
    Box::new(bsn! {
        Node {
            width: px(24),
            height: px(24),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Button
        PlayButton
        Children [
            Node {
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            PlayGlyph(false)
            Children [ {icons::play()} ]
            --
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            PlayGlyph(true)
            Children [ {icons::close()} ]
        ]
        on(toggle)
        on(over_button)
        on(out_button)
    })
}

/// The hover flyout under the play control. It is a root-level overlay so the
/// menu bar cannot clip it; [`place_flyout`] anchors it to the button.
pub fn flyout() -> Part {
    let row = Box::new(bsn! {
        Node {
            height: px(26),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            column_gap: px(7),
            border_radius: px(3),
        }
        Button
        Children [
            {icons::mono(icons::PLAY_SCENE, tokens::TEXT_MEDIUM)}
            --
            {chrome::text("Play in new window", FONT_SIZE_OUTPUT, tokens::TEXT)}
        ]
        on(play_in_window_row)
    });

    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: px(2),
            border: px(1),
            border_radius: px(4),
            min_width: px(184),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        GlobalZIndex(100)
        OverrideClip
        PlayFlyout
        Children [ {row} ]
        on(over_flyout)
        on(out_flyout)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    fn scene() -> SceneState {
        let mut state = SceneState::default();
        state.spawn(ObjectKind::Cube);
        state.spawn(ObjectKind::Cube);
        state
    }

    #[test]
    fn the_player_is_launched_with_the_snapshot() {
        let command = player_command(
            Path::new("/bin/eme_player"),
            Path::new("/cache/play/scene.ember"),
            Path::new("/workspace"),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--scene", "/cache/play/scene.ember"]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/workspace")));
    }

    #[test]
    fn stopping_without_a_child_is_a_no_op() {
        let mut play = PlayState::default();
        play.stop();
        assert!(!play.running());
    }

    #[test]
    fn embedded_play_restores_the_document_after_edits() {
        let mut state = scene();
        let before = state.doc.to_ron().unwrap();
        let mut view = ViewMode::Editor;
        let mut play = PlayState::default();

        play.start_embedded(&mut state, &mut view);
        assert_eq!(view, ViewMode::Game);
        assert!(play.running());

        state.spawn(ObjectKind::Sphere);
        state.doc.rename(1, "Edited while playing");
        assert_ne!(state.doc.to_ron().unwrap(), before);

        play.stop_embedded(&mut state, &mut view);
        assert_eq!(state.doc.to_ron().unwrap(), before);
        assert_eq!(view, ViewMode::Editor);
        assert!(!play.running());
    }

    #[test]
    fn selection_and_anchor_survive_a_play_stop_round_trip() {
        let mut state = scene();
        state.select_many(&[1, 2]);
        let mut view = ViewMode::Editor;
        let mut play = PlayState::default();

        play.start_embedded(&mut state, &mut view);
        state.clear_selection();
        state.select_only(2);
        play.stop_embedded(&mut state, &mut view);

        assert_eq!(state.selection.items, vec![1, 2]);
        assert_eq!(state.selection.primary, Some(2));
        assert_eq!(state.anchor, Some(1));
    }

    #[test]
    fn the_flyout_shows_while_either_part_is_hovered() {
        let mut hover = PlayHover::default();
        assert!(!hover.visible());

        hover.button = true;
        assert!(hover.visible());
        hover.button = false;
        hover.flyout = true;
        assert!(hover.visible());
        hover.flyout = false;
        assert!(!hover.visible());
    }

    #[test]
    fn activating_the_flyout_hides_it_until_the_pointer_leaves() {
        let mut hover = PlayHover {
            flyout: true,
            dismissed: true,
            ..default()
        };
        assert!(!hover.shown());

        hover.flyout = false;
        assert!(!hover.visible());
        hover.dismissed = false;
        hover.button = true;
        assert!(hover.shown());
    }

    #[test]
    fn a_windowed_game_is_stopped_instead_of_starting_embedded_play() {
        assert_eq!(next(true, false), Toggle::StopWindowed);
        assert_eq!(next(true, true), Toggle::StopWindowed);
        assert_eq!(next(false, true), Toggle::StopEmbedded);
        assert_eq!(next(false, false), Toggle::StartEmbedded);
    }
}
