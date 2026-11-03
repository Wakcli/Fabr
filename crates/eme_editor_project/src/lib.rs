//! On-disk project format, project creation, and persisted editor state.

/// Project creation and opening.
pub mod create;
/// Project errors.
pub mod error;
/// Manifest format and conversion to editor projects.
pub mod manifest;
/// Recent projects, layout, and theme file state on disk.
pub mod recents;

pub use create::{create_project, open_project};
pub use error::ProjectError;
pub use manifest::{Manifest, MANIFEST_FILE, SCHEMA_VERSION};
pub use recents::{
    load_session_state, load_user_state, save_session_state, save_user_state, user_config_dir,
    SessionState,
};
