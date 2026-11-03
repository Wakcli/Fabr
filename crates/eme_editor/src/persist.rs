//! Scene save/load: the editor document to and from `scenes/main.ember`.
//!
//! The file is plain RON from `eme_game_scene`, so a runtime player can read the
//! same bytes without any editor crate.

use core::fmt;
use std::path::{Path, PathBuf};

use eme::prelude::*;
use eme_game_scene::{SceneDoc, SceneError};

use crate::scene::SceneState;

/// Where Ctrl+S writes and Ctrl+O reads.
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

/// Writes `doc` to [`SCENE_PATH`], creating the `scenes` folder if needed.
pub fn save(doc: &SceneDoc) -> Result<PathBuf, PersistError> {
    let path = PathBuf::from(SCENE_PATH);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| io(dir, err))?;
    }
    let text = doc.to_ron()?;
    std::fs::write(&path, text).map_err(|err| io(&path, err))?;
    Ok(path)
}

/// Reads [`SCENE_PATH`], or `None` when no scene has been saved yet.
pub fn load() -> Result<Option<SceneDoc>, PersistError> {
    let path = Path::new(SCENE_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let src = std::fs::read_to_string(path).map_err(|err| io(path, err))?;
    Ok(Some(SceneDoc::from_ron(&src)?))
}

/// Replaces the editor document with the saved scene, if it exists.
pub fn open_into(state: &mut SceneState) {
    match load() {
        Ok(Some(doc)) => {
            let count = doc.len();
            state.replace_doc(doc);
            info!("loaded {count} objects from {SCENE_PATH}");
        }
        Ok(None) => warn!("no scene at {SCENE_PATH}"),
        Err(err) => error!("could not load {SCENE_PATH}: {err}"),
    }
}

/// Ctrl+S saves the document, Ctrl+O loads it from disk.
pub fn scene_shortcuts(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<SceneState>) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    if keys.just_pressed(KeyCode::KeyS) {
        match save(&state.doc) {
            Ok(path) => info!("saved scene to {}", path.display()),
            Err(err) => error!("could not save {SCENE_PATH}: {err}"),
        }
    }
    if keys.just_pressed(KeyCode::KeyO) {
        open_into(&mut state);
    }
}
