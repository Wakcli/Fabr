//! Scene document state for the editor: hierarchy, undoable edits, save/load, and play mode.

mod commands;
mod doc;
mod play;

pub use commands::{DespawnEntity, Rename, SetTranslation, SpawnEntity};
pub use doc::{DocState, TreeItem};

use core::fmt;

/// Error raised while opening, saving, or restoring a document.
#[derive(Debug)]
pub enum DocError {
    /// [`DocState::save`] was called before a path was set.
    NoPath,
    /// Reading or writing the scene file failed.
    Io(std::io::Error),
    /// RON encoding failed.
    Ron(ron::Error),
    /// RON decoding failed.
    Parse(ron::error::SpannedError),
    /// The decoded world holds a type the registry cannot apply.
    Spawn(eme_world_serialization::WorldInstanceSpawnError),
}

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPath => f.write_str("scene has no path yet; use save_as"),
            Self::Io(err) => write!(f, "scene io: {err}"),
            Self::Ron(err) => write!(f, "scene ron: {err}"),
            Self::Parse(err) => write!(f, "scene parse: {err}"),
            Self::Spawn(err) => write!(f, "scene spawn: {err}"),
        }
    }
}

impl std::error::Error for DocError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoPath => None,
            Self::Io(err) => Some(err),
            Self::Ron(err) => Some(err),
            Self::Parse(err) => Some(err),
            Self::Spawn(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for DocError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ron::Error> for DocError {
    fn from(err: ron::Error) -> Self {
        Self::Ron(err)
    }
}

impl From<ron::error::SpannedError> for DocError {
    fn from(err: ron::error::SpannedError) -> Self {
        Self::Parse(err)
    }
}

impl From<eme_world_serialization::WorldInstanceSpawnError> for DocError {
    fn from(err: eme_world_serialization::WorldInstanceSpawnError) -> Self {
        Self::Spawn(err)
    }
}
