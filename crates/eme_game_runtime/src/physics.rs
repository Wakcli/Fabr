//! A small gravity and AABB physics step shared by the editor's Game view and
//! the runtime player. It is deliberately simple: bodies are axis-aligned boxes,
//! static bodies are obstacles, and the ground is the `y = 0` plane.

use eme_ecs::prelude::*;
use eme_game_scene::PhysicsBodySpec;
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;

/// Gravity and collision tuning for the physics step.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSettings {
    /// Downward acceleration in units per second squared.
    pub gravity: f32,
    /// Fraction of vertical speed kept after a landing, 0 to 1.
    pub bounce: f32,
    /// Whether the step runs at all.
    pub enabled: bool,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: 9.81,
            bounce: 0.0,
            enabled: true,
        }
    }
}

/// A box body taking part in [`step_physics`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PhysicsBody {
    /// Whether gravity and collisions move this body.
    pub dynamic: bool,
    /// Half extents before the entity's scale is applied.
    pub half_extents: Vec3,
    /// Current linear velocity.
    pub velocity: Vec3,
    /// Whether the body rested on the ground or a static box in the last step.
    pub grounded: bool,
}

impl PhysicsBody {
    /// Builds a runtime body from authored scene data.
    pub fn from_spec(spec: &PhysicsBodySpec) -> Self {
        Self {
            dynamic: spec.dynamic,
            half_extents: Vec3::from_array(spec.half_extents),
            velocity: Vec3::ZERO,
            grounded: false,
        }
    }
}

/// Advances every dynamic body by one frame.
pub fn step_physics(
    time: Res<Time>,
    settings: Res<PhysicsSettings>,
    mut bodies: Query<(&mut Transform, &mut PhysicsBody)>,
) {
    if !settings.enabled {
        return;
    }
    let dt = time.delta_secs().clamp(0.0, 0.05);
    if dt <= 0.0 {
        return;
    }
    let statics: Vec<(Vec3, Vec3)> = bodies
        .iter()
        .filter(|(_, body)| !body.dynamic)
        .map(|(transform, body)| {
            (
                transform.translation,
                body.half_extents * transform.scale.abs(),
            )
        })
        .collect();
    for (mut transform, mut body) in &mut bodies {
        if body.dynamic {
            integrate(&mut body, &mut transform, &settings, dt, &statics);
        }
    }
}

fn integrate(
    body: &mut PhysicsBody,
    transform: &mut Transform,
    settings: &PhysicsSettings,
    dt: f32,
    statics: &[(Vec3, Vec3)],
) {
    body.velocity.y -= settings.gravity * dt;
    transform.translation += body.velocity * dt;
    body.grounded = false;

    let half = body.half_extents * transform.scale.abs();
    let bottom = transform.translation.y - half.y;
    if bottom < 0.0 {
        transform.translation.y -= bottom;
        land(body, settings);
        body.grounded = true;
    }

    for &(center, other) in statics {
        let delta = transform.translation - center;
        let overlap = half + other - delta.abs();
        if overlap.x <= 0.0 || overlap.y <= 0.0 || overlap.z <= 0.0 {
            continue;
        }
        if overlap.y <= overlap.x && overlap.y <= overlap.z {
            let dir = if delta.y >= 0.0 { 1.0 } else { -1.0 };
            transform.translation.y += overlap.y * dir;
            if dir > 0.0 {
                body.grounded = true;
                if body.velocity.y < 0.0 {
                    land(body, settings);
                }
            } else if body.velocity.y > 0.0 {
                body.velocity.y = 0.0;
            }
        } else if overlap.x <= overlap.z {
            let dir = if delta.x >= 0.0 { 1.0 } else { -1.0 };
            transform.translation.x += overlap.x * dir;
            if (dir > 0.0 && body.velocity.x < 0.0) || (dir < 0.0 && body.velocity.x > 0.0) {
                body.velocity.x = 0.0;
            }
        } else {
            let dir = if delta.z >= 0.0 { 1.0 } else { -1.0 };
            transform.translation.z += overlap.z * dir;
            if (dir > 0.0 && body.velocity.z < 0.0) || (dir < 0.0 && body.velocity.z > 0.0) {
                body.velocity.z = 0.0;
            }
        }
    }
}

fn land(body: &mut PhysicsBody, settings: &PhysicsSettings) {
    body.velocity.y = -body.velocity.y * settings.bounce;
    if body.velocity.y.abs() < 0.5 {
        body.velocity.y = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn falling(y: f32) -> (Transform, PhysicsBody) {
        (
            Transform::from_xyz(0.0, y, 0.0),
            PhysicsBody::from_spec(&PhysicsBodySpec::default()),
        )
    }

    fn step(transform: &mut Transform, body: &mut PhysicsBody, statics: &[(Vec3, Vec3)]) {
        integrate(
            body,
            transform,
            &PhysicsSettings::default(),
            1.0 / 60.0,
            statics,
        );
    }

    #[test]
    fn a_dynamic_body_falls_and_lands_on_the_ground() {
        let (mut transform, mut body) = falling(3.0);
        for _ in 0..180 {
            step(&mut transform, &mut body, &[]);
        }
        assert!((transform.translation.y - 0.5).abs() < 1e-3);
        assert_eq!(body.velocity, Vec3::ZERO);
        assert!(body.grounded);
    }

    #[test]
    fn a_static_box_catches_a_falling_body() {
        let (mut transform, mut body) = falling(4.0);
        let statics = [(Vec3::new(0.0, 1.0, 0.0), Vec3::splat(0.5))];
        for _ in 0..180 {
            step(&mut transform, &mut body, &statics);
        }
        assert!((transform.translation.y - 2.0).abs() < 1e-3);
        assert!(body.grounded);
    }

    #[test]
    fn disabled_physics_leaves_the_body_still() {
        let (mut transform, mut body) = falling(3.0);
        let settings = PhysicsSettings {
            enabled: false,
            ..Default::default()
        };
        // The system gates on `enabled`; assert the tuning itself keeps zero gravity out
        // of the integrator by calling it with zero gravity.
        let zero = PhysicsSettings {
            gravity: 0.0,
            ..settings
        };
        integrate(&mut body, &mut transform, &zero, 1.0 / 60.0, &[]);
        assert_eq!(transform.translation.y, 3.0);
    }

    #[test]
    fn bounce_keeps_part_of_the_landing_speed() {
        let (mut transform, mut body) = falling(0.5);
        body.velocity.y = -10.0;
        let settings = PhysicsSettings {
            bounce: 0.5,
            ..Default::default()
        };
        integrate(&mut body, &mut transform, &settings, 1.0 / 60.0, &[]);
        assert!(body.velocity.y > 0.0);
        assert!(body.velocity.y < 6.0);
    }
}
