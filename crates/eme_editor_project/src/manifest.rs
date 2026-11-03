use std::fs;
use std::path::{Path, PathBuf};

use eme_editor_core::{Project, ProjectId};
use ron::ser::{to_string_pretty, PrettyConfig};
use serde::{Deserialize, Serialize};

use crate::error::ProjectError;

/// File name of the manifest inside a project root.
pub const MANIFEST_FILE: &str = "emberrly.project.ron";
/// Manifest schema written by this crate.
pub const SCHEMA_VERSION: u32 = 1;

pub(crate) const ASSET_ROOT: &str = "assets";

/// Contents of [`MANIFEST_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version.
    pub schema: u32,
    /// Stable project identifier.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Engine version that last wrote the project.
    pub engine_version: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Asset directory relative to the project root.
    pub asset_root: String,
    /// Scene opened on startup, relative to the project root.
    pub entry_scene: Option<String>,
    /// Freeform tags.
    pub tags: Vec<String>,
}

impl Manifest {
    /// Reads and parses the manifest at `path`.
    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let src = fs::read_to_string(path)?;
        ron::from_str(&src).map_err(|e| ProjectError::Parse(e.to_string()))
    }

    /// Writes the manifest to `path`.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let body = to_string_pretty(self, PrettyConfig::default())
            .map_err(|e| ProjectError::Parse(e.to_string()))?;
        fs::write(path, body)?;
        Ok(())
    }

    /// Builds a manifest for `project`, recording `engine_version`.
    ///
    /// `created` is taken from [`Project::last_opened`]; project creation
    /// overwrites it with the creation time.
    pub fn from_project(project: &Project, engine_version: &str) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            id: project.id.0,
            name: project.name.clone(),
            engine_version: engine_version.to_string(),
            created: project.last_opened.unwrap_or(0),
            asset_root: ASSET_ROOT.to_string(),
            entry_scene: None,
            tags: Vec::new(),
        }
    }

    /// Builds a project rooted at `root`; [`Project::last_opened`] is left for the caller.
    pub fn to_project(&self, root: PathBuf) -> Project {
        Project {
            id: ProjectId(self.id),
            name: self.name.clone(),
            root,
            last_opened: None,
            engine_version: self.engine_version.clone(),
        }
    }
}
