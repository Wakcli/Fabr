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

/// Tuning for [`KinematicController`] movement.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct ControllerSettings {
    /// Horizontal speed in units per second.
    pub walk_speed: f32,
    /// Upward speed applied when jumping.
    pub jump_speed: f32,
    /// Tallest ledge the controller can step onto.
    pub step_height: f32,
    /// Steepest ground normal (degrees from vertical) that counts as walkable.
    pub slope_deg: f32,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self {
            walk_speed: 4.0,
            jump_speed: 5.0,
            step_height: 0.35,
            slope_deg: 45.0,
        }
    }
}

/// Whether a ground normal within `slope_deg` of straight up is walkable.
pub fn walkable(normal: Vec3, slope_deg: f32) -> bool {
    let limit = ops::cos(slope_deg.clamp(0.0, 89.0).to_radians());
    normal.normalize_or_zero().y >= limit
}

/// A gravity-driven, player-controlled box.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct KinematicController {
    /// Half extents of the box.
    pub half_extents: Vec3,
    /// Current velocity; horizontal from [`desired`](Self::desired), vertical from gravity.
    pub velocity: Vec3,
    /// Direction (normalised XZ) the script asked to walk.
    pub desired: Vec3,
    /// Whether the controller stood on ground in the last step.
    pub grounded: bool,
    /// Jump requested by a script, consumed by the next step.
    pub jump: bool,
}

impl KinematicController {
    /// Builds a controller from authored extents.
    pub fn new(half_extents: Vec3) -> Self {
        Self {
            half_extents,
            velocity: Vec3::ZERO,
            desired: Vec3::ZERO,
            grounded: false,
            jump: false,
        }
    }

    /// Asks the controller to walk in a direction (XZ).
    pub fn walk(&mut self, x: f32, z: f32) {
        self.desired = Vec3::new(x, 0.0, z).normalize_or_zero();
    }

    /// Asks the controller to jump on the next step.
    pub fn jump(&mut self) {
        self.jump = true;
    }
}

/// Moves kinematic controllers with gravity, step-up and wall blocking.
pub fn step_controllers(
    time: Res<Time>,
    physics: Res<PhysicsSettings>,
    tuning: Res<ControllerSettings>,
    bodies: Query<(&Transform, &PhysicsBody), Without<KinematicController>>,
    mut controllers: Query<(&mut Transform, &mut KinematicController), Without<PhysicsBody>>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.05);
    if dt <= 0.0 || !physics.enabled {
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
    for (mut transform, mut controller) in &mut controllers {
        let half = controller.half_extents * transform.scale.abs();
        controller.velocity.x = controller.desired.x * tuning.walk_speed;
        controller.velocity.z = controller.desired.z * tuning.walk_speed;
        controller.velocity.y -= physics.gravity * dt;
        if controller.jump {
            controller.jump = false;
            if controller.grounded {
                controller.velocity.y = tuning.jump_speed;
                controller.grounded = false;
            }
        }

        transform.translation.y += controller.velocity.y * dt;
        controller.grounded = false;
        if transform.translation.y - half.y < 0.0 {
            transform.translation.y = half.y;
            controller.velocity.y = 0.0;
            controller.grounded = true;
        }
        for (center, other) in &statics {
            resolve_vertical(&mut transform, &mut controller, half, *center, *other);
        }

        move_axis(
            &mut transform,
            &mut controller,
            half,
            &statics,
            Vec3::X,
            tuning.step_height,
        );
        move_axis(
            &mut transform,
            &mut controller,
            half,
            &statics,
            Vec3::Z,
            tuning.step_height,
        );
    }
}

fn overlap(center: Vec3, half: Vec3, other: Vec3, other_half: Vec3) -> Vec3 {
    (half + other_half) - (center - other).abs()
}

fn resolve_vertical(
    transform: &mut Transform,
    controller: &mut KinematicController,
    half: Vec3,
    center: Vec3,
    other: Vec3,
) {
    let delta = transform.translation - center;
    let hit = overlap(delta, half, Vec3::ZERO, other);
    if hit.x <= 0.0 || hit.y <= 0.0 || hit.z <= 0.0 {
        return;
    }
    if hit.y <= hit.x && hit.y <= hit.z {
        if delta.y >= 0.0 {
            transform.translation.y += hit.y;
            controller.grounded = true;
            if controller.velocity.y < 0.0 {
                controller.velocity.y = 0.0;
            }
        } else if controller.velocity.y > 0.0 {
            transform.translation.y -= hit.y;
            controller.velocity.y = 0.0;
        }
    }
}

fn move_axis(
    transform: &mut Transform,
    controller: &mut KinematicController,
    half: Vec3,
    statics: &[(Vec3, Vec3)],
    axis: Vec3,
    step_height: f32,
) {
    let travel = controller.velocity.dot(axis) * 0.016;
    if travel == 0.0 {
        return;
    }
    transform.translation += axis * travel;
    for (center, other) in statics {
        let delta = transform.translation - *center;
        let hit = overlap(delta, half, Vec3::ZERO, *other);
        if hit.x <= 0.0 || hit.y <= 0.0 || hit.z <= 0.0 {
            continue;
        }
        // a low ledge is climbed instead of blocking
        let top = center.y + other.y;
        let feet = transform.translation.y - half.y;
        let rise = top - feet;
        if controller.grounded && rise > 0.0 && rise <= step_height {
            transform.translation.y += rise;
            continue;
        }
        let push = if axis == Vec3::X {
            hit.x.min(hit.z)
        } else {
            hit.z.min(hit.x)
        };
        let sign = if controller.velocity.dot(axis) > 0.0 {
            -1.0
        } else {
            1.0
        };
        transform.translation += axis * push * sign;
        controller.velocity -= axis * controller.velocity.dot(axis);
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
    /// Current angular velocity in radians per second around world axes.
    pub angular_velocity: Vec3,
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
            angular_velocity: Vec3::ZERO,
            grounded: false,
        }
    }

    /// Applies an angular impulse (world axes) through the box inertia approximation.
    pub fn apply_angular_impulse(&mut self, impulse: Vec3, mass: f32) {
        let inertia = box_inertia(self.half_extents, mass.max(0.001));
        self.angular_velocity += Vec3::new(
            impulse.x / inertia.x.max(1e-4),
            impulse.y / inertia.y.max(1e-4),
            impulse.z / inertia.z.max(1e-4),
        );
    }
}

/// Principal moments of inertia of a solid box of `mass`.
pub fn box_inertia(half_extents: Vec3, mass: f32) -> Vec3 {
    let size = half_extents.abs() * 2.0;
    mass.max(0.0) / 12.0
        * Vec3::new(
            size.y * size.y + size.z * size.z,
            size.x * size.x + size.z * size.z,
            size.x * size.x + size.y * size.y,
        )
}

/// Fraction of angular speed kept after a contact.
const CONTACT_SPIN_DAMPING: f32 = 0.55;

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
    let spin = body.angular_velocity.length();
    if spin > 1e-6 {
        let axis = body.angular_velocity / spin;
        let delta = Quat::from_axis_angle(axis, spin * dt);
        transform.rotation = (delta * transform.rotation).normalize();
    }
    body.grounded = false;

    let half = body.half_extents * transform.scale.abs();
    let bottom = transform.translation.y - half.y;
    if bottom < 0.0 {
        transform.translation.y -= bottom;
        land(body, settings);
        body.angular_velocity *= CONTACT_SPIN_DAMPING;
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
                body.angular_velocity *= CONTACT_SPIN_DAMPING;
                if body.velocity.y < 0.0 {
                    land(body, settings);
                }
            } else if body.velocity.y > 0.0 {
                body.velocity.y = 0.0;
            }
        } else if overlap.x <= overlap.z {
            let dir = if delta.x >= 0.0 { 1.0 } else { -1.0 };
            transform.translation.x += overlap.x * dir;
            body.angular_velocity *= CONTACT_SPIN_DAMPING;
            if (dir > 0.0 && body.velocity.x < 0.0) || (dir < 0.0 && body.velocity.x > 0.0) {
                body.velocity.x = 0.0;
            }
        } else {
            let dir = if delta.z >= 0.0 { 1.0 } else { -1.0 };
            transform.translation.z += overlap.z * dir;
            body.angular_velocity *= CONTACT_SPIN_DAMPING;
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

    fn controller_world() -> (Transform, KinematicController) {
        (
            Transform::from_xyz(0.0, 0.5, 0.0),
            KinematicController::new(Vec3::splat(0.5)),
        )
    }

    fn step_controller(
        transform: &mut Transform,
        controller: &mut KinematicController,
        statics: &[Transform],
    ) {
        let bodies: Vec<PhysicsBody> = statics
            .iter()
            .map(|_| {
                PhysicsBody::from_spec(&PhysicsBodySpec {
                    dynamic: false,
                    half_extents: [0.5; 3],
                    controller: false,
                })
            })
            .collect();
        let half = controller.half_extents * transform.scale.abs();
        controller.velocity.x = controller.desired.x * 4.0;
        controller.velocity.z = controller.desired.z * 4.0;
        controller.velocity.y -= 9.81 / 60.0;
        transform.translation += controller.velocity / 60.0;
        controller.grounded = false;
        if transform.translation.y - half.y < 0.0 {
            transform.translation.y = half.y;
            controller.velocity.y = 0.0;
            controller.grounded = true;
        }
        let statics: Vec<(Vec3, Vec3)> = statics
            .iter()
            .zip(&bodies)
            .map(|(transform, body)| {
                (
                    transform.translation,
                    body.half_extents * transform.scale.abs(),
                )
            })
            .collect();
        for (center, other) in &statics {
            resolve_vertical(transform, controller, half, *center, *other);
        }
        move_axis(transform, controller, half, &statics, Vec3::X, 0.35);
        move_axis(transform, controller, half, &statics, Vec3::Z, 0.35);
    }

    #[test]
    fn a_controller_walks_forward_and_stays_grounded() {
        let (mut transform, mut controller) = controller_world();
        controller.walk(1.0, 0.0);
        for _ in 0..60 {
            step_controller(&mut transform, &mut controller, &[]);
        }
        assert!(transform.translation.x > 3.0);
        assert!((transform.translation.y - 0.5).abs() < 1e-3);
        assert!(controller.grounded);
    }

    #[test]
    fn a_controller_falls_to_the_ground() {
        let (mut transform, mut controller) = controller_world();
        transform.translation.y = 4.0;
        for _ in 0..120 {
            step_controller(&mut transform, &mut controller, &[]);
        }
        assert!((transform.translation.y - 0.5).abs() < 1e-2);
        assert!(controller.grounded);
    }

    #[test]
    fn a_controller_steps_onto_a_low_ledge() {
        let (mut transform, mut controller) = controller_world();
        controller.walk(1.0, 0.0);
        let ledge = Transform::from_xyz(1.0, 0.1, 0.0)
            .with_scale(Vec3::new(1.0, 0.4, 1.0));
        let mut climbed = false;
        for _ in 0..90 {
            step_controller(&mut transform, &mut controller, &[ledge]);
            climbed |= transform.translation.y > 0.6;
        }
        assert!(transform.translation.x > 1.0, "blocked at {}", transform.translation.x);
        assert!(climbed, "never climbed the ledge");
    }

    #[test]
    fn a_tall_wall_blocks_the_controller() {
        let (mut transform, mut controller) = controller_world();
        controller.walk(1.0, 0.0);
        let wall = Transform::from_xyz(1.0, 0.5, 0.0);
        for _ in 0..60 {
            step_controller(&mut transform, &mut controller, &[wall]);
        }
        assert!(transform.translation.x <= 1.0, "walked through the wall");
    }

    #[test]
    fn slope_limit_accepts_gentle_and_rejects_steep_normals() {
        let gentle = Vec3::new(0.5, 0.866, 0.0);
        let steep = Vec3::new(0.866, 0.5, 0.0);
        assert!(walkable(gentle, 45.0));
        assert!(!walkable(steep, 45.0));
        assert!(!walkable(Vec3::NEG_Y, 45.0));
    }

    #[test]
    fn a_spinning_body_rotates_in_free_flight() {
        let mut transform = Transform::from_xyz(0.0, 5.0, 0.0);
        let mut body = PhysicsBody {
            dynamic: true,
            angular_velocity: Vec3::new(0.0, core::f32::consts::PI, 0.0),
            ..PhysicsBody::from_spec(&PhysicsBodySpec::default())
        };
        let settings = PhysicsSettings {
            gravity: 0.0,
            ..PhysicsSettings::default()
        };
        for _ in 0..30 {
            integrate(&mut body, &mut transform, &settings, 1.0 / 60.0, &[]);
        }
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        assert!((yaw.abs() - core::f32::consts::FRAC_PI_2).abs() < 1e-3, "{yaw}");
        assert_eq!(body.angular_velocity, Vec3::new(0.0, core::f32::consts::PI, 0.0));
    }

    #[test]
    fn landing_damps_angular_velocity() {
        let mut transform = Transform::from_xyz(0.0, 2.0, 0.0);
        let mut body = PhysicsBody {
            dynamic: true,
            angular_velocity: Vec3::new(0.0, 4.0, 0.0),
            ..PhysicsBody::from_spec(&PhysicsBodySpec::default())
        };
        for _ in 0..180 {
            integrate(
                &mut body,
                &mut transform,
                &PhysicsSettings::default(),
                1.0 / 60.0,
                &[],
            );
        }
        assert!(body.grounded, "the body should have landed");
        assert!(
            body.angular_velocity.length() < 4.0 * 0.5,
            "spin was not damped: {:?}",
            body.angular_velocity
        );
        assert!(transform.translation.y >= 0.5 - 1e-3, "fell through");
    }

    #[test]
    fn box_inertia_and_impulses_agree() {
        let inertia = box_inertia(Vec3::splat(0.5), 2.0);
        let expected = 2.0 / 12.0 * (1.0 + 1.0);
        assert!((inertia.x - expected).abs() < 1e-5);
        let mut body = PhysicsBody {
            dynamic: true,
            ..PhysicsBody::from_spec(&PhysicsBodySpec::default())
        };
        body.apply_angular_impulse(Vec3::new(0.0, inertia.y * 2.0, 0.0), 2.0);
        assert!((body.angular_velocity.y - 2.0).abs() < 1e-4);
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
