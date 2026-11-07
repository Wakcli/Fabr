//! Behaviour runtime shared by the editor's Instance view and the player.
//!
//! The authored document stores the base transform. [`SceneBehaviour`] keeps that
//! base beside the entity and [`advance`] derives the transform for an absolute
//! time, so a behaviour never writes back into the document.

use eme_ecs::prelude::*;
use std::path::{Path, PathBuf};

use eme_game_scene::{Behaviour, BehaviourKind};
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;

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
pub fn run_behaviours(time: Res<Time>, mut objects: Query<(&SceneBehaviour, &mut Transform)>) {
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

/// Finds the model file whose sidecar carries `guid`, under `assets`.
///
/// Sidecars are `<model>.import.ron`; the model path is the sidecar path with that
/// suffix removed. The walk is bounded so a huge asset folder cannot stall a spawn.
pub fn find_by_guid(assets: &Path, guid: &str) -> Option<PathBuf> {
    find_in(assets, guid, 0)
}

fn find_in(dir: &Path, guid: &str, depth: usize) -> Option<PathBuf> {
    if depth > 6 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
            continue;
        }
        if path.to_string_lossy().ends_with(".import.ron")
            && sidecar_guid(&path).as_deref() == Some(guid)
        {
            let name = path.file_name()?.to_string_lossy();
            let model = name.strip_suffix(".import.ron")?;
            return Some(path.with_file_name(model));
        }
    }
    for dir in dirs {
        if let Some(found) = find_in(&dir, guid, depth + 1) {
            return Some(found);
        }
    }
    None
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

#[cfg(test)]
mod asset_tests {
    use super::*;

    #[test]
    fn a_guid_finds_its_model_after_a_rename() {
        let dir = std::env::temp_dir().join(format!("fabr-guid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let model = dir.join("models/moved.glb");
        std::fs::write(&model, b"glTF").unwrap();
        std::fs::write(
            dir.join("models/moved.glb.import.ron"),
            "(version: 1, guid: \"abc123\", settings: ())",
        )
        .unwrap();

        assert_eq!(find_by_guid(&dir, "abc123"), Some(model));
        assert_eq!(find_by_guid(&dir, "nope"), None);
    }
}
