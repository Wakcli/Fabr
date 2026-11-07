//! Watches imported models for edits and drives hot reload.
//!
//! A file is only considered changed when its mtime or length moved *and* its
//! content hash differs, so a touch or a re-save without edits is ignored. The
//! reimport runs through [`ImportService`]; a failed reimport leaves the live
//! instance untouched because only successful jobs reach [`apply_reloads`].

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eme::gltf::GltfAssetLabel;
use eme::prelude::*;

use crate::console::ConsoleLog;
use crate::import::{ImportService, Sidecar};
use crate::scene::{ImportPivot, ModelIndex, ModelInstance};

/// What the watcher remembers about one model file.
#[derive(Clone, Copy, Debug)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    hash: u64,
}

/// Model files and the content stamp last seen for each.
#[derive(Resource, Default)]
pub struct ModelWatcher {
    seen: HashMap<PathBuf, Stamp>,
}

impl ModelWatcher {
    /// Finds models whose content changed since the previous poll.
    ///
    /// The first poll records every file without reporting it, so opening a
    /// project does not reimport the whole asset folder.
    pub fn poll(&mut self, root: &Path) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        let mut present = HashSet::new();
        for path in models(root, 0) {
            if !Sidecar::path_for(&path).is_file() {
                continue;
            }
            present.insert(path.clone());
            let Some(meta) = std::fs::metadata(&path).ok() else {
                continue;
            };
            let modified = meta.modified().ok();
            let len = meta.len();
            match self.seen.get(&path).copied() {
                Some(old) if old.modified == modified && old.len == len => {}
                Some(old) => {
                    let Some(stamp) = stamp(&path) else {
                        continue;
                    };
                    self.seen.insert(path.clone(), stamp);
                    if stamp.hash != old.hash {
                        changed.push(path);
                    }
                }
                None => {
                    if let Some(stamp) = stamp(&path) {
                        self.seen.insert(path, stamp);
                    }
                }
            }
        }
        self.seen.retain(|path, _| present.contains(path));
        changed
    }
}

fn models(dir: &Path, depth: usize) -> Vec<PathBuf> {
    if depth > 6 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(models(&path, depth + 1));
            continue;
        }
        let is_model = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf"));
        if is_model {
            found.push(path);
        }
    }
    found
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
        hash: hasher.finish(),
    })
}

/// Queues a background reimport for every model whose content changed.
pub fn watch_models(
    mut watcher: ResMut<ModelWatcher>,
    mut service: ResMut<ImportService>,
    mut log: ResMut<ConsoleLog>,
) {
    let root = crate::asset_root();
    for path in watcher.poll(&root) {
        let rel = path.strip_prefix(&root).map_or_else(
            |_| path.display().to_string(),
            |rel| rel.to_string_lossy().replace('\\', "/"),
        );
        log.push(format!("detected change in {rel}; reimporting"));
        service.reload(&path, &mut log);
    }
}

/// Applies reports from successful hot reimports to the live instances.
pub fn apply_reloads(
    mut service: ResMut<ImportService>,
    mut index: ResMut<ModelIndex>,
    models: Query<(Entity, &ModelInstance, &ChildOf)>,
    mut pivots: Query<&mut Transform, With<ImportPivot>>,
    asset_server: Res<AssetServer>,
    mut log: ResMut<ConsoleLog>,
) {
    if service.reloaded.is_empty() {
        return;
    }
    for path in std::mem::take(&mut service.reloaded) {
        index.0.upsert(&path);
        let Ok(rel) = path.strip_prefix(crate::asset_root()) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let import = eme_game_runtime::read_model_import(&path);
        let pivot = eme_game_runtime::pivot_transform(import);
        let guid = Sidecar::load(&path).map_or_else(String::new, |sidecar| sidecar.guid);
        let mut refreshed = 0;
        for (_entity, model, parent) in &models {
            if model.path != rel && (guid.is_empty() || model.guid != guid) {
                continue;
            }
            if let Ok(mut transform) = pivots.get_mut(parent.parent()) {
                *transform = pivot;
            }
            refreshed += 1;
        }
        asset_server.reload(GltfAssetLabel::Scene(0).from_asset(rel.clone()));
        log.push(format!(
            "reimported {rel}; refreshed {refreshed} instance(s)"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn model(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::write(
            Sidecar::path_for(&path),
            "(version: 1, guid: \"g\", settings: ())",
        )
        .unwrap();
        path
    }

    fn touch(path: &Path, when: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn an_unchanged_hash_is_skipped() {
        let dir = scratch("hash");
        let path = model(&dir, "chair.glb", b"glTF first");
        let mut watcher = ModelWatcher::default();
        assert!(watcher.poll(&dir).is_empty());

        touch(&path, SystemTime::now() + std::time::Duration::from_secs(5));
        assert!(watcher.poll(&dir).is_empty());

        std::fs::write(&path, b"glTF second").unwrap();
        touch(
            &path,
            SystemTime::now() + std::time::Duration::from_secs(10),
        );
        assert_eq!(watcher.poll(&dir), vec![path.clone()]);
        assert!(watcher.poll(&dir).is_empty());
    }

    #[test]
    fn models_without_a_sidecar_are_ignored() {
        let dir = scratch("plain");
        std::fs::write(dir.join("loose.glb"), b"glTF").unwrap();
        let mut watcher = ModelWatcher::default();
        assert!(watcher.poll(&dir).is_empty());
        std::fs::write(dir.join("loose.glb"), b"glTF changed").unwrap();
        assert!(watcher.poll(&dir).is_empty());
    }
}
