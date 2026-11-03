use core::fmt;
use std::io;

/// Failure while creating, opening, or persisting editor project state.
#[derive(Debug)]
pub enum ProjectError {
    /// Filesystem failure.
    Io(io::Error),
    /// Malformed manifest or state document.
    Parse(String),
    /// Manifest was written by a schema newer than this editor supports.
    UnsupportedSchema(u32),
    /// Project name rejected by validation.
    InvalidName(String),
    /// Manifest names an asset root that does not exist.
    MissingAssetRoot,
    /// Manifest names an entry scene that does not exist.
    MissingEntryScene,
    /// Directory holds no project manifest.
    NotAProject,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "project io error: {err}"),
            Self::Parse(err) => write!(f, "could not parse project state: {err}"),
            Self::UnsupportedSchema(schema) => {
                write!(f, "project schema {schema} is newer than supported")
            }
            Self::InvalidName(name) => write!(f, "invalid project name `{name}`"),
            Self::MissingAssetRoot => f.write_str("project asset root is missing"),
            Self::MissingEntryScene => f.write_str("project entry scene is missing"),
            Self::NotAProject => f.write_str("directory does not contain a project manifest"),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ProjectError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}
