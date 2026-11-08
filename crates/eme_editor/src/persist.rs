//! Scene save/load: the editor document to and from `scenes/main.ember`.
//!
//! The file is plain RON from `eme_game_scene`, so a runtime player can read the
//! same bytes without any editor crate.

use core::fmt;
use std::path::{Path, PathBuf};

use eme::input_focus::InputFocus;
use eme::prelude::*;
use eme::text::EditableText;
use eme_game_scene::{SceneDoc, SceneError};

use crate::app::Screen;
use crate::build::BuildState;
use crate::clipboard::{self, Clipboard};
use crate::console::ConsoleLog;
use crate::scene::{ModelIndex, SceneState};
use crate::undo::Undo;

/// The main scene tab's path, and the scene Ctrl+O reads by default.
pub const SCENE_PATH: &str = "scenes/main.ember";

/// Why a scene file could not be written or read back.
#[derive(Debug)]
pub enum PersistError {
    Io(String),
    Scene(SceneError),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistError::Io(err) => write!(f, "{err}"),
            PersistError::Scene(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PersistError {}

impl From<SceneError> for PersistError {
    fn from(err: SceneError) -> Self {
        PersistError::Scene(err)
    }
}

fn io(path: &Path, err: std::io::Error) -> PersistError {
    PersistError::Io(format!("{}: {err}", path.display()))
}

/// Writes `doc` to `path`, creating its folder if needed.
pub fn save_to(doc: &SceneDoc, path: &str) -> Result<PathBuf, PersistError> {
    let path = PathBuf::from(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| io(dir, err))?;
    }
    let text = doc.to_ron()?;
    std::fs::write(&path, text).map_err(|err| io(&path, err))?;
    Ok(path)
}

/// Reads [`SCENE_PATH`], or `None` when no scene has been saved yet.
pub fn load() -> Result<Option<SceneDoc>, PersistError> {
    load_from(SCENE_PATH)
}

/// Reads `path`, or `None` when no scene has been saved there yet.
pub fn load_from(path: &str) -> Result<Option<SceneDoc>, PersistError> {
    let path = Path::new(path);
    if !path.exists() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(path).map_err(|err| io(path, err))?;
    Ok(Some(SceneDoc::from_ron(&src)?))
}

/// Replaces the editor document with the saved scene at `path`, if it exists.
pub fn open_path_into(
    state: &mut SceneState,
    path: &str,
    index: &ModelIndex,
    log: &mut ConsoleLog,
) {
    match load_from(path) {
        Ok(Some(mut doc)) => {
            crate::import::migrate_doc(
                &mut doc,
                &crate::asset_root(),
                &index.0,
                log,
            );
            let count = doc.len();
            state.replace_doc(doc);
            info!("loaded {count} objects from {path}");
        }
        Ok(None) => warn!("no scene at {path}"),
        Err(err) => error!("could not load {path}: {err}"),
    }
}

/// Undo, redo, and the clipboard shortcuts.
///
/// Runs only on the editor screen, and never while a text field owns the keyboard
/// or the build dialog is up.
pub fn edit_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<Screen>,
    build: Res<BuildState>,
    focus: Res<InputFocus>,
    texts: Query<(), With<EditableText>>,
    mut undo: ResMut<Undo>,
    mut clip: ResMut<Clipboard>,
    mut state: ResMut<SceneState>,
) {
    if *screen != Screen::Editor || build.open {
        return;
    }
    if let Some(entity) = focus.get()
        && texts.contains(entity)
    {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            if undo.can_redo() {
                undo.redo(&mut state);
            }
        } else if undo.can_undo() {
            undo.undo(&mut state);
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyY) {
        if undo.can_redo() {
            undo.redo(&mut state);
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        clipboard::copy_selection(&state, &mut clip);
        return;
    }
    if keys.just_pressed(KeyCode::KeyX) {
        clipboard::cut_selection(&mut state, &mut clip, &mut undo);
        return;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        clipboard::paste(&mut state, &clip, &mut undo);
        return;
    }
    if keys.just_pressed(KeyCode::KeyD) {
        clipboard::duplicate_selection(&mut state, &mut undo);
    }
}
