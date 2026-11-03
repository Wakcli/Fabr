use std::fs;
use std::io;
use std::path::Path;

use eme_editor_core::{Project, ProjectId};

use crate::error::ProjectError;
use crate::manifest::{Manifest, ASSET_ROOT, MANIFEST_FILE, SCHEMA_VERSION};

const MAX_NAME: usize = 64;
const GITIGNORE: &str = "target/\n";

/// Creates a project rooted at `root` and returns its [`Project`].
///
/// No scene file is written, so the manifest records no entry scene. A non-empty
/// existing directory is rejected unless `overwrite` is set.
pub fn create_project(
    root: &Path,
    name: &str,
    engine_version: &str,
    now: u64,
    overwrite: bool,
) -> Result<Project, ProjectError> {
    validate_name(name)?;
    prepare_root(root, overwrite)?;

    fs::create_dir_all(root.join(ASSET_ROOT))?;
    fs::create_dir_all(root.join("scenes"))?;

    let manifest = Manifest {
        schema: SCHEMA_VERSION,
        id: id_for(root),
        name: name.to_string(),
        engine_version: engine_version.to_string(),
        created: now,
        asset_root: ASSET_ROOT.to_string(),
        entry_scene: None,
        tags: Vec::new(),
    };
    manifest.save(&root.join(MANIFEST_FILE))?;
    fs::write(root.join(".gitignore"), GITIGNORE)?;

    Ok(Project {
        id: ProjectId(manifest.id),
        name: name.to_string(),
        root: root.to_path_buf(),
        last_opened: Some(now),
        engine_version: engine_version.to_string(),
    })
}

/// Reads and validates the project at `root`.
///
/// Fails when the manifest is absent, newer than [`SCHEMA_VERSION`], or points at
/// an asset root or entry scene that is gone.
pub fn open_project(root: &Path) -> Result<(Project, Manifest), ProjectError> {
    let path = root.join(MANIFEST_FILE);
    if !path.is_file() {
        return Err(ProjectError::NotAProject);
    }
    let manifest = Manifest::load(&path)?;
    if manifest.schema > SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedSchema(manifest.schema));
    }
    if !root.join(&manifest.asset_root).exists() {
        return Err(ProjectError::MissingAssetRoot);
    }
    if let Some(scene) = &manifest.entry_scene
        && !root.join(scene).exists()
    {
        return Err(ProjectError::MissingEntryScene);
    }
    Ok((manifest.to_project(root.to_path_buf()), manifest))
}

fn validate_name(name: &str) -> Result<(), ProjectError> {
    let bad = name.is_empty()
        || name.chars().count() > MAX_NAME
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\');
    if bad {
        return Err(ProjectError::InvalidName(name.to_string()));
    }
    Ok(())
}

fn prepare_root(root: &Path, overwrite: bool) -> Result<(), ProjectError> {
    if !root.exists() {
        return Ok(());
    }
    if !root.is_dir() {
        return Err(already_exists(root, "is not a directory"));
    }
    if overwrite {
        return Ok(());
    }
    if fs::read_dir(root)?.next().is_some() {
        return Err(already_exists(root, "is not empty"));
    }
    Ok(())
}

fn already_exists(root: &Path, what: &str) -> ProjectError {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("`{}` {what}", root.display()),
    )
    .into()
}

fn id_for(root: &Path) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
