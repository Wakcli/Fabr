//! Behaviour runtime shared by the editor's Instance view and the player.
//!
//! The authored document stores the base transform. [`SceneBehaviour`] keeps that
//! base beside the entity and [`advance`] derives the transform for an absolute
//! time, so a behaviour never writes back into the document.

extern crate alloc;

use eme_ecs::prelude::*;
use std::path::{Path, PathBuf};

use eme_game_scene::{Behaviour, BehaviourKind};
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;

/// A small gravity and AABB physics step shared by the editor and player.
pub mod physics;
/// Grid navigation, A* paths and path-following agents.
pub mod nav;
/// Optional engine extensions toggled by `fabr.plugins.ron`.
pub mod extensions;

pub use extensions::{load_for, Extension, ExtensionSet, PLUGINS_FILE};
pub use nav::{
    NavAgent, NavGrid, NavGridSettings, NavPlugin, NavSet, refresh_nav_grid, step_nav_agents,
};
pub use physics::{
    step_controllers, step_physics, walkable, ControllerSettings, KinematicController, PhysicsBody,
    PhysicsSettings,
};

/// The authored behaviour plus the transform it advances from.
#[derive(Component, Clone, Copy, Debug)]
pub struct SceneBehaviour {
    /// Authored behaviour for this object.
    pub spec: Behaviour,
    /// Base transform taken from the document.
    pub base: Transform,
}

/// The transform `spec` produces `time` seconds after `base`.
///
/// `Spin` composes around the world Y axis; `Bob` offsets along world Y and
/// leaves rotation and scale untouched. `None` returns `base` unchanged.
pub fn advance(spec: &Behaviour, base: &Transform, time: f32) -> Transform {
    match spec.kind {
        BehaviourKind::None => *base,
        BehaviourKind::Spin => Transform {
            rotation: Quat::from_rotation_y(spec.speed * time) * base.rotation,
            ..*base
        },
        BehaviourKind::Bob => Transform {
            translation: base.translation + Vec3::Y * (spec.amount * ops::sin(spec.speed * time)),
            ..*base
        },
    }
}

/// Writes every behaviour's transform for the current elapsed time.
pub fn run_behaviours(
    time: Res<Time>,
    mut objects: Query<
        (&SceneBehaviour, &mut Transform),
        (
            Without<PhysicsBody>,
            Without<NavAgent>,
            Without<KinematicController>,
        ),
    >,
) {
    let now = time.elapsed_secs();
    for (behaviour, mut transform) in &mut objects {
        *transform = advance(&behaviour.spec, &behaviour.base, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spinning() -> Behaviour {
        Behaviour {
            kind: BehaviourKind::Spin,
            speed: 2.0,
            amount: 0.25,
        }
    }

    fn bobbing() -> Behaviour {
        Behaviour {
            kind: BehaviourKind::Bob,
            speed: 2.0,
            amount: 0.5,
        }
    }

    fn base() -> Transform {
        Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_x(0.3))
    }

    #[test]
    fn spin_starts_at_the_base_transform() {
        let base = base();
        assert_eq!(advance(&spinning(), &base, 0.0), base);
    }

    #[test]
    fn spin_turns_half_way_at_pi_over_speed() {
        let base = base();
        let turned = advance(&spinning(), &base, core::f32::consts::PI / 2.0);
        let forward = turned.rotation * Vec3::NEG_Z;
        let want = (Quat::from_rotation_y(core::f32::consts::PI) * base.rotation) * Vec3::NEG_Z;
        assert!((forward - want).length() < 1e-5, "{forward:?} != {want:?}");
        assert_eq!(turned.translation, base.translation);
        assert_eq!(turned.scale, base.scale);
    }

    #[test]
    fn bob_peaks_at_a_quarter_cycle() {
        let base = base();
        let top = advance(&bobbing(), &base, core::f32::consts::FRAC_PI_4);
        assert!(ops::abs(top.translation.y - (base.translation.y + 0.5)) < 1e-5);
        let middle = advance(&bobbing(), &base, core::f32::consts::PI / 2.0);
        assert!(ops::abs(middle.translation.y - base.translation.y) < 1e-5);
        assert_eq!(middle.rotation, base.rotation);
    }

    #[test]
    fn none_returns_the_base_transform() {
        let base = base();
        let idle = Behaviour::default();
        assert_eq!(advance(&idle, &base, 12.5), base);
    }
}

/// Index from sidecar GUID to the model file that carries it.
///
/// Built once per asset root and updated when a sidecar appears or changes, so a
/// scene resolve is a map lookup instead of a walk over `assets/`.
#[derive(Clone, Debug, Default)]
pub struct GuidIndex {
    root: PathBuf,
    by_guid: std::collections::HashMap<String, PathBuf>,
}

impl GuidIndex {
    /// Walks `root` and records every sidecar it finds.
    pub fn scan(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut index = Self {
            root: root.clone(),
            by_guid: std::collections::HashMap::new(),
        };
        index.rebuild();
        index
    }

    /// The asset root this index was built from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Forgets everything and walks the asset root again.
    pub fn rebuild(&mut self) {
        self.by_guid.clear();
        collect_sidecars(&self.root, 0, &mut self.by_guid);
    }

    /// The model file whose sidecar carries `guid`.
    pub fn get(&self, guid: &str) -> Option<&Path> {
        self.by_guid.get(guid).map(PathBuf::as_path)
    }

    /// Records `model` again after its sidecar changed on disk.
    pub fn upsert(&mut self, model: &Path) {
        let Some(guid) = sidecar_guid(&sidecar_path(model)) else {
            return;
        };
        self.by_guid.retain(|_, path| path != model);
        self.by_guid.insert(guid, model.to_path_buf());
    }

    /// Drops `model` from the index.
    pub fn remove(&mut self, model: &Path) {
        self.by_guid.retain(|_, path| path != model);
    }

    /// Number of indexed models.
    pub fn len(&self) -> usize {
        self.by_guid.len()
    }

    /// True when no sidecar was found under the asset root.
    pub fn is_empty(&self) -> bool {
        self.by_guid.is_empty()
    }
}

fn collect_sidecars(
    dir: &Path,
    depth: usize,
    out: &mut std::collections::HashMap<String, PathBuf>,
) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(model_name) = name.strip_suffix(SIDECAR_SUFFIX) else {
            continue;
        };
        if let Some(guid) = sidecar_guid(&path) {
            out.insert(guid, path.with_file_name(model_name));
        }
    }
    for dir in dirs {
        collect_sidecars(&dir, depth + 1, out);
    }
}

fn sidecar_path(model: &Path) -> PathBuf {
    let name = model
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    model.with_file_name(format!("{name}{SIDECAR_SUFFIX}"))
}

fn sidecar_guid(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let at = text.find("guid:")? + "guid:".len();
    let rest = &text[at..];
    let open = rest.find('"')? + 1;
    let after = &rest[open..];
    let close = after.find('"')?;
    Some(after[..close].to_owned())
}

/// Suffix shared with the editor's per-asset import sidecar.
pub const SIDECAR_SUFFIX: &str = ".import.ron";

/// Which source axis the model treats as up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpAxis {
    /// glTF's own convention.
    #[default]
    Y,
    /// Blender's default export when not converted.
    Z,
}

/// The imported-model settings that affect how an instance is placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelImport {
    /// Uniform scale in metres per source unit.
    pub scale: f32,
    /// Source up axis.
    pub up_axis: UpAxis,
}

impl Default for ModelImport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            up_axis: UpAxis::Y,
        }
    }
}

/// Reads scale and up-axis from a model's sidecar, falling back to identity.
pub fn read_model_import(model: &Path) -> ModelImport {
    let Ok(text) = std::fs::read_to_string(sidecar_path(model)) else {
        return ModelImport::default();
    };
    let scale = read_number(&text, "scale").unwrap_or(1.0);
    let up_axis = match read_word(&text, "up_axis").as_deref() {
        Some("Z") => UpAxis::Z,
        _ => UpAxis::Y,
    };
    ModelImport { scale, up_axis }
}

fn read_number(text: &str, key: &str) -> Option<f32> {
    let at = text.find(key)? + key.len();
    let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn read_word(text: &str, key: &str) -> Option<String> {
    let at = text.find(key)? + key.len();
    let rest = text[at..].trim_start().strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_owned())
}

/// The transform the import settings put on the pivot between object and model.
pub fn pivot_transform(import: ModelImport) -> Transform {
    let rotation = match import.up_axis {
        UpAxis::Y => Quat::IDENTITY,
        UpAxis::Z => Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2),
    };
    Transform::from_scale(Vec3::splat(import.scale)).with_rotation(rotation)
}

#[cfg(test)]
mod asset_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-guid-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        dir
    }

    #[test]
    fn a_guid_finds_its_model_after_a_rename() {
        let dir = scratch("rename");
        let model = dir.join("models/moved.glb");
        std::fs::write(&model, b"glTF").unwrap();
        std::fs::write(
            sidecar_path(&model),
            "(version: 1, guid: \"abc123\", settings: (transform: (scale: 1.0, up_axis: Y)))",
        )
        .unwrap();

        let index = GuidIndex::scan(&dir);
        assert_eq!(index.get("abc123"), Some(model.as_path()));
        assert_eq!(index.get("nope"), None);
    }

    #[test]
    fn a_new_sidecar_is_indexed_without_a_rescan() {
        let dir = scratch("upsert");
        let model = dir.join("models/late.glb");
        std::fs::write(&model, b"glTF").unwrap();
        let mut index = GuidIndex::scan(&dir);
        assert!(index.is_empty());

        std::fs::write(
            sidecar_path(&model),
            "(version: 1, guid: \"late-guid\", settings: ())",
        )
        .unwrap();
        index.upsert(&model);
        assert_eq!(index.get("late-guid"), Some(model.as_path()));
    }

    #[test]
    fn a_sidecar_supplies_the_pivot_transform() {
        let dir = scratch("pivot");
        let model = dir.join("models/blob.glb");
        std::fs::write(&model, b"glTF").unwrap();
        std::fs::write(
            sidecar_path(&model),
            r#"(version: 1, guid: "g", settings: (transform: (scale: 2.5, up_axis: Z)))"#,
        )
        .unwrap();

        let import = read_model_import(&model);
        assert_eq!(import.scale, 2.5);
        assert_eq!(import.up_axis, UpAxis::Z);
        let pivot = pivot_transform(import);
        assert_eq!(pivot.scale, Vec3::splat(2.5));
        let tilted = pivot.rotation * Vec3::Y;
        assert!((tilted - Vec3::NEG_Z).length() < 1e-5, "{tilted:?}");
    }

    #[test]
    fn a_missing_sidecar_keeps_the_identity_pivot() {
        let dir = scratch("missing");
        let model = dir.join("models/plain.glb");
        std::fs::write(&model, b"glTF").unwrap();
        assert_eq!(read_model_import(&model), ModelImport::default());
    }
}

pub use eme_game_scene::NodeKey;

/// One key, built the same way by the importer and by the scene tagger.
pub fn extend(parent: &[u32], ordinal: usize, name: &str) -> NodeKey {
    NodeKey {
        path: parent.to_vec(),
        name: String::new(),
    }
    .child(ordinal, name)
}

/// The key for a model root.
pub fn root_key(name: &str) -> NodeKey {
    NodeKey::root(name)
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn keys_carry_path_and_name() {
        let a = extend(&[], 2, "Cube");
        assert_eq!(a.path, vec![2]);
        assert_eq!(a.name, "Cube");
        let b = extend(&a.path, 0, "Cube");
        assert_eq!(b.path, vec![2, 0]);
    }

    #[test]
    fn the_same_ordinal_with_a_new_name_is_a_different_key() {
        assert_ne!(extend(&[], 1, "Cube"), extend(&[], 1, "Chair"));
    }
}
