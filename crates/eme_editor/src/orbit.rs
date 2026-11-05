//! Viewport camera: an orbit rig for inspecting the scene and a first-person fly
//! rig for moving through it, toggled with `Tab`.

use eme::input::mouse::MouseScrollUnit;
use eme::input_focus::InputFocus;
use eme::math::ops;
use eme::prelude::*;
use eme::text::EditableText;

use crate::scene::{SceneState, ViewportCamera};

/// Starting eye position, kept in sync with the editor's default perspective.
const HOME_EYE: Vec3 = Vec3::new(7.0, 5.0, 7.0);
/// Rotation per dragged pixel.
const SENSITIVITY: f32 = 0.3 * core::f32::consts::PI / 180.0;
/// How far the camera may look up or down before the up axis degenerates.
const PITCH_LIMIT: f32 = 85.0 * core::f32::consts::PI / 180.0;
/// Multiplier per scroll notch.
const ZOOM_STEP: f32 = 1.1;
const MIN_DISTANCE: f32 = 2.0;
const MAX_DISTANCE: f32 = 40.0;
/// World units panned per pixel, as a fraction of the orbit distance.
const PAN_SPEED: f32 = 0.0015;
/// X11 reports line scroll; pixel-mode devices get divided down to one.
const PIXELS_PER_NOTCH: f32 = 50.0;
/// Fly speed at the start of a hold, world units per second.
const FLY_BASE: f32 = 3.5;
/// Extra speed gained per second of continuous movement.
const FLY_RAMP: f32 = 6.0;
/// Upper bound so a long hold stays controllable.
const FLY_MAX: f32 = 34.0;
/// Speed range for the first-person rig, world units per second.
const FLY_MIN_SPEED: f32 = 0.5;
const FLY_MAX_SPEED: f32 = 60.0;
/// World units panned per pixel in first-person mode.
const EYE_PAN_SPEED: f32 = 0.002;

/// Where the viewport camera looks from, expressed as an orbit around a target.
#[derive(Resource, Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        let offset = HOME_EYE;
        let distance = offset.length();
        Self {
            yaw: ops::atan2(offset.x, offset.z),
            pitch: ops::asin(offset.y / distance),
            distance,
            target: Vec3::ZERO,
        }
    }
}

/// Fly state for right-mouse + WASD navigation.
#[derive(Resource, Default)]
pub struct FlyMove {
    /// Seconds spent continuously moving during the current right-hold.
    pub held: f32,
    /// Set when a right-hold was used to fly, so releasing it does not open a menu.
    pub suppress_menu: bool,
}

/// Which rig drives the viewport camera.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    Fly,
    Orbit,
}

/// Editor preview vs the scene camera's game view.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    #[default]
    Editor,
    Game,
}

/// Free-flying eye used while [`CameraMode::Fly`] is active.
#[derive(Resource)]
pub struct FlyCam {
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

impl Default for FlyCam {
    fn default() -> Self {
        Self {
            eye: HOME_EYE,
            yaw: ops::atan2(HOME_EYE.x, HOME_EYE.z),
            pitch: -ops::asin(HOME_EYE.y / HOME_EYE.length()),
            speed: 8.0,
        }
    }
}

/// `Tab` swaps rigs, carrying the current eye and heading across so the view never jumps.
pub fn toggle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    texts: Query<(), With<EditableText>>,
    cams: Query<&GlobalTransform, With<ViewportCamera>>,
    mut mode: ResMut<CameraMode>,
    mut fly: ResMut<FlyCam>,
    mut orbit: ResMut<OrbitCamera>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    if let Some(entity) = focus.get()
        && texts.contains(entity)
    {
        return;
    }
    let Ok(cam) = cams.single() else {
        return;
    };

    let (_, rotation, eye) = cam.to_scale_rotation_translation();
    match *mode {
        CameraMode::Orbit => {
            let (yaw, pitch, _) = rotation.to_euler(EulerRot::YXZ);
            fly.eye = eye;
            fly.yaw = yaw;
            fly.pitch = pitch;
            *mode = CameraMode::Fly;
        }
        CameraMode::Fly => {
            orbit.yaw = fly.yaw;
            orbit.pitch = -fly.pitch;
            let forward = rotation * -Vec3::Z;
            orbit.target = eye + forward * orbit.distance;
            *mode = CameraMode::Orbit;
        }
    }
}

/// Right-mouse + WASD moves the orbit target; speed ramps while movement continues.
pub fn fly_move(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mode: Res<CameraMode>,
    mut orbit: ResMut<OrbitCamera>,
    mut cam: ResMut<FlyCam>,
    mut fly: ResMut<FlyMove>,
) {
    if mouse.just_pressed(MouseButton::Right) {
        fly.held = 0.0;
        fly.suppress_menu = false;
    }
    if !mouse.pressed(MouseButton::Right) {
        fly.held = 0.0;
        return;
    }

    let forward = match *mode {
        CameraMode::Orbit => {
            let (sy, cy) = (ops::sin(orbit.yaw), ops::cos(orbit.yaw));
            let (sp, cp) = (ops::sin(orbit.pitch), ops::cos(orbit.pitch));
            -Vec3::new(cp * sy, sp, cp * cy)
        }
        CameraMode::Fly => {
            let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
            rot * -Vec3::Z
        }
    };
    let right = forward.cross(Vec3::Y).normalize_or_zero();

    let mut dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        dir += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        dir -= Vec3::Y;
    }
    if dir == Vec3::ZERO {
        fly.held = 0.0;
        return;
    }

    fly.held = (fly.held + time.delta_secs()).min(8.0);
    let covered = match *mode {
        CameraMode::Orbit => {
            let speed = (FLY_BASE + FLY_RAMP * fly.held).min(FLY_MAX);
            dir.normalize() * speed * time.delta_secs()
        }
        CameraMode::Fly => {
            let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                4.0
            } else {
                1.0
            };
            dir.normalize() * cam.speed * boost * time.delta_secs()
        }
    };

    match *mode {
        CameraMode::Orbit => orbit.target += covered,
        CameraMode::Fly => cam.eye += covered,
    }
    fly.suppress_menu = true;
}

/// Drives the camera transform from whichever rig is active.
pub fn apply_camera(
    view: Res<ViewMode>,
    state: Res<SceneState>,
    objects: Query<&GlobalTransform>,
    mode: Res<CameraMode>,
    orbit: Res<OrbitCamera>,
    cam: Res<FlyCam>,
    mut cams: Query<&mut Transform, With<ViewportCamera>>,
) {
    let Ok(mut transform) = cams.single_mut() else {
        return;
    };
    if *view == ViewMode::Game {
        let shot = state
            .doc
            .objects
            .iter()
            .find(|obj| obj.kind == eme_game_scene::ObjectKind::Camera)
            .and_then(|obj| state.entities.get(&obj.id))
            .and_then(|entity| objects.get(*entity).ok());
        if let Some(global) = shot {
            *transform =
                Transform::from_translation(global.translation()).with_rotation(global.rotation());
            return;
        }
    }
    match *mode {
        CameraMode::Orbit => {
            let (sy, cy) = (ops::sin(orbit.yaw), ops::cos(orbit.yaw));
            let (sp, cp) = (ops::sin(orbit.pitch), ops::cos(orbit.pitch));
            let offset = Vec3::new(cp * sy, sp, cp * cy) * orbit.distance;
            let eye = orbit.target + offset;
            *transform = Transform::from_translation(eye).looking_at(orbit.target, Vec3::Y);
        }
        CameraMode::Fly => {
            *transform = Transform::from_translation(cam.eye).with_rotation(Quat::from_euler(
                EulerRot::YXZ,
                cam.yaw,
                cam.pitch,
                0.0,
            ));
        }
    }
}

fn look(cam: &mut FlyCam, delta: Vec2) {
    cam.yaw -= delta.x * SENSITIVITY;
    cam.pitch = (cam.pitch - delta.y * SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
}

fn pan_eye(cam: &mut FlyCam, delta: Vec2) {
    let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let scale = cam.speed * EYE_PAN_SPEED;
    cam.eye -= rot * Vec3::X * delta.x * scale;
    cam.eye += rot * Vec3::Y * delta.y * scale;
}

fn rotate(orbit: &mut OrbitCamera, delta: Vec2) {
    orbit.yaw -= delta.x * SENSITIVITY;
    orbit.pitch = (orbit.pitch + delta.y * SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
}

fn pan(orbit: &mut OrbitCamera, delta: Vec2, cam: &GlobalTransform) {
    let scale = orbit.distance * PAN_SPEED;
    orbit.target -= *cam.right() * delta.x * scale;
    orbit.target += *cam.up() * delta.y * scale;
}

pub(crate) fn orbit_drag(
    drag: On<PointerDrag>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<CameraMode>,
    cams: Query<&GlobalTransform, With<ViewportCamera>>,
    mut orbit: ResMut<OrbitCamera>,
    mut cam: ResMut<FlyCam>,
) {
    if *mode == CameraMode::Fly {
        match drag.button {
            PointerButton::Secondary => look(&mut cam, drag.delta),
            PointerButton::Middle => pan_eye(&mut cam, drag.delta),
            PointerButton::Primary => {}
        }
        return;
    }

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    match drag.button {
        PointerButton::Secondary => rotate(&mut orbit, drag.delta),
        PointerButton::Middle => {
            if shift {
                if let Ok(cam) = cams.single() {
                    pan(&mut orbit, drag.delta, cam);
                }
            } else {
                rotate(&mut orbit, drag.delta);
            }
        }
        PointerButton::Primary => {}
    }
}

pub(crate) fn orbit_scroll(
    scroll: On<PointerScroll>,
    mode: Res<CameraMode>,
    mut orbit: ResMut<OrbitCamera>,
    mut cam: ResMut<FlyCam>,
) {
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.y,
        MouseScrollUnit::Pixel => scroll.y / PIXELS_PER_NOTCH,
    };
    if notches == 0.0 {
        return;
    }
    match *mode {
        CameraMode::Orbit => {
            orbit.distance =
                (orbit.distance * ops::powf(ZOOM_STEP, -notches)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
        CameraMode::Fly => {
            cam.speed =
                (cam.speed * ops::powf(ZOOM_STEP, notches)).clamp(FLY_MIN_SPEED, FLY_MAX_SPEED);
        }
    }
}

/// `F`/`Home` returns the viewport to the default perspective.
pub fn reset_orbit(
    keys: Res<ButtonInput<KeyCode>>,
    mut orbit: ResMut<OrbitCamera>,
    mut cam: ResMut<FlyCam>,
) {
    if keys.just_pressed(KeyCode::KeyF) || keys.just_pressed(KeyCode::Home) {
        *orbit = OrbitCamera::default();
        *cam = FlyCam::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_matches_home_eye() {
        let orbit = OrbitCamera::default();
        let (sy, cy) = (ops::sin(orbit.yaw), ops::cos(orbit.yaw));
        let (sp, cp) = (ops::sin(orbit.pitch), ops::cos(orbit.pitch));
        let eye = orbit.target + Vec3::new(cp * sy, sp, cp * cy) * orbit.distance;
        assert!((eye - HOME_EYE).length() < 1e-4, "eye was {eye:?}");
    }

    #[test]
    fn pitch_stops_short_of_the_poles() {
        let mut orbit = OrbitCamera::default();
        rotate(&mut orbit, Vec2::new(0.0, 100_000.0));
        assert!((orbit.pitch - PITCH_LIMIT).abs() < 1e-6);
        rotate(&mut orbit, Vec2::new(0.0, -200_000.0));
        assert!((orbit.pitch + PITCH_LIMIT).abs() < 1e-6);
    }

    #[test]
    fn dragging_right_orbits_the_eye_left_of_the_scene() {
        let mut orbit = OrbitCamera::default();
        let before = orbit.yaw;
        rotate(&mut orbit, Vec2::new(10.0, 0.0));
        assert!(orbit.yaw < before);
    }

    #[test]
    fn default_fly_eye_looks_at_the_origin() {
        let cam = FlyCam::default();
        let forward = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0) * -Vec3::Z;
        let want = (-cam.eye).normalize();
        assert!((forward - want).length() < 1e-4, "forward was {forward:?}");
    }

    #[test]
    fn fly_look_stops_short_of_the_poles() {
        let mut cam = FlyCam::default();
        look(&mut cam, Vec2::new(0.0, 100_000.0));
        assert!((cam.pitch + PITCH_LIMIT).abs() < 1e-6);
        look(&mut cam, Vec2::new(0.0, -200_000.0));
        assert!((cam.pitch - PITCH_LIMIT).abs() < 1e-6);
    }

    #[test]
    fn the_editor_starts_in_first_person() {
        assert_eq!(CameraMode::default(), CameraMode::Fly);
    }

    #[test]
    fn looking_never_slides_the_eye() {
        let mut cam = FlyCam::default();
        let eye = cam.eye;
        look(&mut cam, Vec2::new(120.0, -40.0));
        assert_eq!(cam.eye, eye, "turning moved the eye instead of the view");
    }

    #[test]
    fn middle_drag_pans_the_eye_sideways() {
        let mut cam = FlyCam::default();
        let before = cam.eye;
        pan_eye(&mut cam, Vec2::new(10.0, 0.0));
        let moved = cam.eye - before;
        let forward = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0) * -Vec3::Z;
        assert!(moved.length() > 0.0);
        assert!(
            moved.dot(forward).abs() < 1e-4,
            "pan leaked into the view axis"
        );
    }
}
