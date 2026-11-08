//! Watches `assets/scripts` and asks the script runtime to reload changed files.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eme::prelude::*;
use eme_script::ScriptReload;

#[derive(Clone, Copy)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    hash: u64,
}

/// Script files and the stamp last seen for each.
#[derive(Resource, Default)]
pub struct ScriptWatcher {
    seen: HashMap<PathBuf, Stamp>,
}

impl ScriptWatcher {
    fn poll(&mut self, root: &Path) -> Vec<String> {
        let mut changed = Vec::new();
        let mut present = HashSet::new();
        for path in scripts(root, 0) {
            present.insert(path.clone());
            let Ok(meta) = std::fs::metadata(&path) else {
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
                    if stamp.hash != old.hash
                        && let Ok(rel) = path.strip_prefix(root)
                    {
                        changed.push(format!("scripts/{}", rel.to_string_lossy().replace('\\', "/")));
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

fn scripts(dir: &Path, depth: usize) -> Vec<PathBuf> {
    if depth > 4 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(scripts(&path, depth + 1));
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lua"))
        {
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

/// Requests a reload for every script whose content changed.
pub fn watch_scripts(mut watcher: ResMut<ScriptWatcher>, mut reload: ResMut<ScriptReload>) {
    let root = crate::asset_root().join("scripts");
    for path in watcher.poll(&root) {
        info!("script changed: {path}");
        reload.request(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-scripts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_changed_scripts_are_reported() {
        let dir = scratch("watch");
        let path = dir.join("spin.lua");
        std::fs::write(&path, "return {}").unwrap();
        let mut watcher = ScriptWatcher::default();
        assert!(watcher.poll(&dir).is_empty());
        std::fs::write(&path, "return { }").unwrap();
        let changed = watcher.poll(&dir);
        assert_eq!(changed, vec!["scripts/spin.lua".to_string()]);
        assert!(watcher.poll(&dir).is_empty());
    }
}
