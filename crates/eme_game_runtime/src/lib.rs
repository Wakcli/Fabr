//! Behaviour runtime shared by the editor's Instance view and the player.
//!
//! The authored document stores the base transform. [`SceneBehaviour`] keeps that
//! base beside the entity and [`advance`] derives the transform for an absolute
//! time, so a behaviour never writes back into the document.

use eme_ecs::prelude::*;
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
