//! Compares the node list of one import with the previous one.
//!
//! The previous list is cached in the git-ignored target directory, keyed by the
//! asset GUID so a rename inside the project still compares against itself.

use std::path::{Path, PathBuf};

use eme_game_scene::NodeKey;

use super::Sidecar;

/// What changed between two imported node lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeDiff {
    /// Nodes that appeared.
    pub added: Vec<NodeKey>,
    /// Nodes that disappeared.
    pub removed: Vec<NodeKey>,
    /// Nodes that kept their name but changed path.
    pub moved: Vec<(NodeKey, NodeKey)>,
    /// Nodes that kept their path but changed name.
    pub renamed: Vec<(NodeKey, NodeKey)>,
    /// True when there was no cached list before this import.
    pub first: bool,
}

impl NodeDiff {
    /// One line per change; empty on the first import.
    pub fn summary_lines(&self) -> Vec<String> {
        if self.first {
            return Vec::new();
        }
        let mut lines = Vec::new();
        for key in &self.added {
            lines.push(format!("node added: {}", label(key)));
        }
        for key in &self.removed {
            lines.push(format!("node removed: {}", label(key)));
        }
        for (from, to) in &self.moved {
            lines.push(format!("node moved: {} → {}", label(from), label(to)));
        }
        for (from, to) in &self.renamed {
            lines.push(format!("node renamed: {} → {}", label(from), label(to)));
        }
        lines
    }

    /// Compact text for the inspector.
    pub fn headline(&self) -> String {
        if self.first {
            return "first import".into();
        }
        if self.added.is_empty()
            && self.removed.is_empty()
            && self.moved.is_empty()
            && self.renamed.is_empty()
        {
            return "no node changes".into();
        }
        format!(
            "+{} −{} moved {} renamed {}",
            self.added.len(),
            self.removed.len(),
            self.moved.len(),
            self.renamed.len()
        )
    }
}

/// Where the cached list for `asset` lives.
fn cache_file(asset: &Path) -> PathBuf {
    let name = Sidecar::load(asset).map_or_else(
        || {
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&asset.to_string_lossy().as_ref(), &mut hasher);
            format!("{:016x}", std::hash::Hasher::finish(&hasher))
        },
        |sidecar| sidecar.guid,
    );
    crate::build::import_cache_dir().join(format!("{name}.nodes.ron"))
}

fn load(asset: &Path) -> Option<Vec<NodeKey>> {
    let text = std::fs::read_to_string(cache_file(asset)).ok()?;
    ron::from_str(&text).ok()
}

fn save(asset: &Path, keys: &[NodeKey]) -> Result<(), String> {
    let path = cache_file(asset);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    let text = ron::ser::to_string(keys).map_err(|err| err.to_string())?;
    std::fs::write(&path, text).map_err(|err| err.to_string())
}

/// Compares `keys` with the cached list, then stores `keys` as the new baseline.
pub fn update(asset: &Path, keys: &[NodeKey]) -> NodeDiff {
    let previous = load(asset);
    let first = previous.is_none();
    let diff = compare(previous.as_deref().unwrap_or_default(), keys);
    let _ = save(asset, keys);
    NodeDiff { first, ..diff }
}

/// Pairs exact matches first, then unique names (moved), then same paths (renamed).
pub fn compare(old: &[NodeKey], new: &[NodeKey]) -> NodeDiff {
    let mut old_left: Vec<NodeKey> = old
        .iter()
        .filter(|key| !new.iter().any(|now| now == *key))
        .cloned()
        .collect();
    let mut new_left: Vec<NodeKey> = new
        .iter()
        .filter(|key| !old.iter().any(|then| then == *key))
        .cloned()
        .collect();

    let mut diff = NodeDiff::default();
    let mut index = 0;
    while index < new_left.len() {
        let key = new_left[index].clone();
        let names: Vec<usize> = new_left
            .iter()
            .enumerate()
            .filter(|(_, other)| other.name == key.name)
            .map(|(at, _)| at)
            .collect();
        let olds: Vec<usize> = old_left
            .iter()
            .enumerate()
            .filter(|(_, other)| other.name == key.name)
            .map(|(at, _)| at)
            .collect();
        if names.len() == 1 && olds.len() == 1 {
            let from = old_left.remove(olds[0]);
            new_left.remove(index);
            diff.moved.push((from, key));
        } else {
            index += 1;
        }
    }

    let mut index = 0;
    while index < new_left.len() {
        let key = new_left[index].clone();
        let olds: Vec<usize> = old_left
            .iter()
            .enumerate()
            .filter(|(_, other)| other.path == key.path)
            .map(|(at, _)| at)
            .collect();
        if olds.len() == 1 {
            let from = old_left.remove(olds[0]);
            new_left.remove(index);
            diff.renamed.push((from, key));
        } else {
            index += 1;
        }
    }

    diff.removed = old_left;
    diff.added = new_left;
    diff
}

fn label(key: &NodeKey) -> String {
    if key.path.is_empty() {
        key.name.clone()
    } else {
        let path = key
            .path
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        format!("{} [{}]", key.name, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(path: &[u32], name: &str) -> NodeKey {
        NodeKey {
            path: path.to_vec(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn a_renamed_node_is_paired_by_path() {
        let old = vec![key(&[0], "Old")];
        let new = vec![key(&[0], "New")];
        let diff = compare(&old, &new);
        assert_eq!(diff.renamed, vec![(key(&[0], "Old"), key(&[0], "New"))]);
        assert!(diff.added.is_empty() && diff.removed.is_empty());
    }

    #[test]
    fn a_moved_node_is_paired_by_unique_name() {
        let old = vec![key(&[0], "Seat")];
        let new = vec![key(&[2], "Seat")];
        let diff = compare(&old, &new);
        assert_eq!(diff.moved, vec![(key(&[0], "Seat"), key(&[2], "Seat"))]);
    }

    #[test]
    fn a_duplicate_name_is_reported_as_add_and_remove() {
        let old = vec![key(&[0], "Leg")];
        let new = vec![key(&[1], "Leg"), key(&[2], "Leg")];
        let diff = compare(&old, &new);
        assert!(diff.moved.is_empty());
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.added.len(), 2);
    }
}
