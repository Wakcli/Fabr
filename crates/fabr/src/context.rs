//! Value-based world access handed to a game's lifecycle hooks.
//!
//! Every method here has a counterpart in the Lua `fabr` table and the C#
//! `FabrScript` base class. Rust can reach the ECS directly through
//! [`FabrGame::build`](crate::FabrGame::build); this surface exists so the same
//! gameplay logic can move between the three languages without changing shape.

use eme::prelude::*;
use eme_anim::AnimationSet;
use eme_game_runtime::{DistanceJoint, KinematicController, NavAgent, NavGrid, PhysicsBody};
use eme_input::prelude::{ButtonInput, KeyCode, MouseButton};
use std::path::PathBuf;

use crate::{
    key_binding, next_layer_id, AudioBus, AudioRequest, InputActions, SceneRequests, ScriptAudio,
    ScriptLog, ScriptRoot, ScriptRuntime, ScriptUi, UiElement,
};

/// The world and the frame's inputs, presented as plain values.
pub struct GameContext<'w> {
    world: &'w mut World,
}

impl<'w> GameContext<'w> {
    pub(crate) fn new(world: &'w mut World) -> Self {
        Self { world }
    }

    /// Seconds elapsed in the last frame.
    pub fn delta(&self) -> f32 {
        self.world
            .get_resource::<Time>()
            .map_or(0.0, Time::delta_secs)
    }

    /// Seconds since the app started.
    pub fn elapsed(&self) -> f32 {
        self.world
            .get_resource::<Time>()
            .map_or(0.0, Time::elapsed_secs)
    }

    /// Whether `name` is held, using the same key names as project settings.
    pub fn key(&self, name: &str) -> bool {
        let Some(code) = key_binding(name) else {
            return false;
        };
        self.world
            .get_resource::<ButtonInput<KeyCode>>()
            .is_some_and(|keys| keys.pressed(code))
    }

    /// Whether `name` went down this frame.
    pub fn key_down(&self, name: &str) -> bool {
        let Some(code) = key_binding(name) else {
            return false;
        };
        self.world
            .get_resource::<ButtonInput<KeyCode>>()
            .is_some_and(|keys| keys.just_pressed(code))
    }

    /// Whether a mouse button is held: 1 left, 2 right, 3 middle.
    pub fn mouse(&self, button: u8) -> bool {
        let button = match button {
            1 => MouseButton::Left,
            2 => MouseButton::Right,
            3 => MouseButton::Middle,
            _ => return false,
        };
        self.world
            .get_resource::<ButtonInput<MouseButton>>()
            .is_some_and(|input| input.pressed(button))
    }

    /// Whether a project input action is held.
    pub fn action(&self, name: &str) -> bool {
        let (Some(actions), Some(keys)) = (
            self.world.get_resource::<InputActions>(),
            self.world.get_resource::<ButtonInput<KeyCode>>(),
        ) else {
            return false;
        };
        actions
            .map
            .get(name)
            .is_some_and(|bindings| bindings.iter().any(|key| keys.pressed(*key)))
    }

    /// Whether a project input action went down this frame.
    pub fn action_down(&self, name: &str) -> bool {
        let (Some(actions), Some(keys)) = (
            self.world.get_resource::<InputActions>(),
            self.world.get_resource::<ButtonInput<KeyCode>>(),
        ) else {
            return false;
        };
        actions
            .map
            .get(name)
            .is_some_and(|bindings| bindings.iter().any(|key| keys.just_pressed(*key)))
    }

    /// Finds the first object with this name.
    pub fn find(&self, name: &str) -> Option<Entity> {
        self.world
            .iter_entities()
            .find(|entity| {
                entity
                    .get::<Name>()
                    .is_some_and(|candidate| candidate.as_str() == name)
            })
            .map(|entity| entity.id())
    }

    /// Name of an object, when it has one.
    pub fn name(&self, entity: Entity) -> Option<&str> {
        self.world.get::<Name>(entity).map(Name::as_str)
    }

    /// World translation.
    pub fn translation(&self, entity: Entity) -> Option<[f32; 3]> {
        self.world
            .get::<Transform>(entity)
            .map(|transform| transform.translation.to_array())
    }

    /// Writes the world translation; false when the object has no transform.
    pub fn set_translation(&mut self, entity: Entity, translation: [f32; 3]) -> bool {
        let Some(mut transform) = self.world.get_mut::<Transform>(entity) else {
            return false;
        };
        transform.translation = Vec3::from_array(translation);
        true
    }

    /// Adds to the world translation.
    pub fn translate(&mut self, entity: Entity, delta: [f32; 3]) -> bool {
        let Some(mut transform) = self.world.get_mut::<Transform>(entity) else {
            return false;
        };
        transform.translation += Vec3::from_array(delta);
        true
    }

    /// World rotation as a quaternion in `[x, y, z, w]` order.
    pub fn rotation(&self, entity: Entity) -> Option<[f32; 4]> {
        self.world
            .get::<Transform>(entity)
            .map(|transform| transform.rotation.to_array())
    }

    /// Writes the world rotation.
    pub fn set_rotation(&mut self, entity: Entity, rotation: [f32; 4]) -> bool {
        let Some(mut transform) = self.world.get_mut::<Transform>(entity) else {
            return false;
        };
        transform.rotation = Quat::from_array(rotation);
        true
    }

    /// Turns an object around the world Y axis.
    pub fn rotate_y(&mut self, entity: Entity, radians: f32) -> bool {
        let Some(mut transform) = self.world.get_mut::<Transform>(entity) else {
            return false;
        };
        transform.rotation = Quat::from_rotation_y(radians) * transform.rotation;
        true
    }

    /// Linear velocity of a physics body.
    pub fn velocity(&self, entity: Entity) -> Option<[f32; 3]> {
        self.world
            .get::<PhysicsBody>(entity)
            .map(|body| body.velocity.to_array())
    }

    /// Writes the linear velocity; false for objects without a physics body.
    pub fn set_velocity(&mut self, entity: Entity, velocity: [f32; 3]) -> bool {
        let Some(mut body) = self.world.get_mut::<PhysicsBody>(entity) else {
            return false;
        };
        body.velocity = Vec3::from_array(velocity);
        if body.velocity.y > 0.0 {
            body.grounded = false;
        }
        true
    }

    /// Whether an object stood on ground in the last physics step.
    pub fn grounded(&self, entity: Entity) -> bool {
        self.world
            .get::<PhysicsBody>(entity)
            .is_some_and(|body| body.grounded)
            || self
                .world
                .get::<KinematicController>(entity)
                .is_some_and(|controller| controller.grounded)
    }

    /// Angular velocity in radians per second around the world axes.
    pub fn angular_velocity(&self, entity: Entity) -> Option<[f32; 3]> {
        self.world
            .get::<PhysicsBody>(entity)
            .map(|body| body.angular_velocity.to_array())
    }

    /// Writes the angular velocity.
    pub fn set_angular_velocity(&mut self, entity: Entity, angular: [f32; 3]) -> bool {
        let Some(mut body) = self.world.get_mut::<PhysicsBody>(entity) else {
            return false;
        };
        body.angular_velocity = Vec3::from_array(angular);
        true
    }

    /// Adds an angular impulse, as if the object had been pushed off-centre.
    pub fn add_spin(&mut self, entity: Entity, impulse: [f32; 3]) -> bool {
        let Some(mut body) = self.world.get_mut::<PhysicsBody>(entity) else {
            return false;
        };
        body.apply_angular_impulse(Vec3::from_array(impulse), 1.0);
        true
    }

    /// Asks a character controller to walk in an XZ direction.
    pub fn walk(&mut self, entity: Entity, x: f32, z: f32) -> bool {
        let Some(mut controller) = self.world.get_mut::<KinematicController>(entity) else {
            return false;
        };
        controller.walk(x, z);
        true
    }

    /// Asks a character controller to jump on the next step.
    pub fn jump(&mut self, entity: Entity) -> bool {
        let Some(mut controller) = self.world.get_mut::<KinematicController>(entity) else {
            return false;
        };
        controller.jump();
        true
    }

    /// Animation clips the object's model provides.
    pub fn animation_clips(&self, entity: Entity) -> Option<&[String]> {
        self.world
            .get::<AnimationSet>(entity)
            .map(|set| set.clips.as_slice())
    }

    /// Plays a clip; false when the model has no such clip.
    pub fn play_animation(&mut self, entity: Entity, name: &str) -> bool {
        let Some(mut set) = self.world.get_mut::<AnimationSet>(entity) else {
            return false;
        };
        set.play(name)
    }

    /// Stops the current clip.
    pub fn stop_animation(&mut self, entity: Entity) -> bool {
        let Some(mut set) = self.world.get_mut::<AnimationSet>(entity) else {
            return false;
        };
        set.stop();
        true
    }

    /// Plans a path between two points on the nav grid.
    pub fn find_path(&self, from: [f32; 3], to: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        let grid = self.world.get_resource::<NavGrid>()?;
        grid.find_path(Vec3::from_array(from), Vec3::from_array(to))
            .map(|path| path.iter().map(Vec3::to_array).collect())
    }

    /// Sends a nav agent to a goal, adding the agent when the object lacks one.
    pub fn move_to(&mut self, entity: Entity, goal: [f32; 3], speed: f32) -> bool {
        let speed = if speed.is_finite() {
            speed.clamp(0.1, 50.0)
        } else {
            3.0
        };
        let goal = Vec3::from_array(goal);
        if let Some(mut agent) = self.world.get_mut::<NavAgent>(entity) {
            agent.speed = speed;
            agent.go_to(goal);
            return true;
        }
        if self.world.get_entity(entity).is_err() {
            return false;
        }
        self.world.entity_mut(entity).insert(NavAgent::default());
        let Some(mut agent) = self.world.get_mut::<NavAgent>(entity) else {
            return false;
        };
        agent.speed = speed;
        agent.go_to(goal);
        true
    }

    /// Stops a nav agent and forgets its goal.
    pub fn stop_moving(&mut self, entity: Entity) -> bool {
        let Some(mut agent) = self.world.get_mut::<NavAgent>(entity) else {
            return false;
        };
        agent.stop();
        true
    }

    /// Queues a sound relative to the asset root; false when the file is absent.
    pub fn play_sound(&mut self, path: &str, volume: f32, bus: AudioBus) -> bool {
        let root = self
            .world
            .get_resource::<ScriptRoot>()
            .map_or_else(|| PathBuf::from("assets"), |root| root.0.clone());
        if !root.join(path).is_file() {
            self.log(format!("script audio: missing asset {path}"));
            return false;
        }
        let Some(mut audio) = self.world.get_resource_mut::<ScriptAudio>() else {
            return false;
        };
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 4.0)
        } else {
            1.0
        };
        audio.requests.push(AudioRequest {
            path: path.to_owned(),
            volume,
            bus,
        });
        true
    }

    /// Removes every HUD element this frame.
    pub fn ui_clear(&mut self) {
        if let Some(mut runtime) = self.world.get_resource_mut::<ScriptRuntime>() {
            runtime.ui.clear();
        }
    }

    /// Draws text at normalised coordinates.
    pub fn ui_text(&mut self, x: f32, y: f32, size: f32, text: impl Into<String>, color: [f32; 3]) {
        let Some(mut runtime) = self.world.get_resource_mut::<ScriptRuntime>() else {
            return;
        };
        runtime.ui.push(UiElement::Text {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            size: size.clamp(4.0, 96.0),
            text: text.into(),
            color,
        });
    }

    /// Draws a filled panel; the rect is `[x, y, width, height]`.
    pub fn ui_panel(&mut self, rect: [f32; 4], color: [f32; 4]) {
        let Some(mut runtime) = self.world.get_resource_mut::<ScriptRuntime>() else {
            return;
        };
        runtime.ui.push(UiElement::Panel {
            x: rect[0].clamp(0.0, 1.0),
            y: rect[1].clamp(0.0, 1.0),
            w: rect[2].clamp(0.0, 1.0),
            h: rect[3].clamp(0.0, 1.0),
            color,
        });
    }

    /// Draws a button; the rect is `[x, y, width, height]`.
    pub fn ui_button(&mut self, id: u32, rect: [f32; 4], label: impl Into<String>) {
        let Some(mut runtime) = self.world.get_resource_mut::<ScriptRuntime>() else {
            return;
        };
        runtime.ui.push(UiElement::Button {
            id,
            x: rect[0].clamp(0.0, 1.0),
            y: rect[1].clamp(0.0, 1.0),
            w: rect[2].clamp(0.0, 1.0),
            h: rect[3].clamp(0.0, 1.0),
            label: label.into(),
        });
    }

    /// Whether a button with this id was pressed since the last frame.
    pub fn ui_clicked(&self, id: u32) -> bool {
        self.world
            .get_resource::<ScriptUi>()
            .is_some_and(|ui| ui.clicks.contains(&id))
    }

    /// Loads a scene document, replacing the current one.
    pub fn load_scene(&mut self, path: &str) -> bool {
        let Some(mut requests) = self.world.get_resource_mut::<SceneRequests>() else {
            return false;
        };
        requests.load = Some(path.to_owned());
        requests.reload = false;
        true
    }

    /// Reloads the current scene document.
    pub fn reload_scene(&mut self) -> bool {
        let Some(mut requests) = self.world.get_resource_mut::<SceneRequests>() else {
            return false;
        };
        requests.reload = true;
        requests.load = None;
        true
    }

    /// Adds a scene document as a layer and returns the layer id.
    pub fn load_scene_additive(&mut self, path: &str) -> Option<u64> {
        let id = next_layer_id();
        let mut requests = self.world.get_resource_mut::<SceneRequests>()?;
        requests.additive.push((id, path.to_owned()));
        Some(id)
    }

    /// Removes an additive scene layer.
    pub fn unload_scene(&mut self, id: u64) -> bool {
        let Some(mut requests) = self.world.get_resource_mut::<SceneRequests>() else {
            return false;
        };
        requests.unload.push(id);
        true
    }

    /// Springs two bodies to a rest distance.
    pub fn create_joint(
        &mut self,
        a: Entity,
        b: Entity,
        rest: f32,
        stiffness: f32,
        damping: f32,
    ) -> bool {
        if self.world.get_entity(a).is_err() || self.world.get_entity(b).is_err() {
            return false;
        }
        self.world.entity_mut(a).insert(DistanceJoint {
            a,
            b,
            rest: rest.max(0.05),
            stiffness: stiffness.clamp(0.0, 200.0),
            damping: damping.clamp(0.0, 50.0),
        });
        true
    }

    /// Writes one line to the script log, falling back to the engine log.
    pub fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        match self.world.get_resource_mut::<ScriptLog>() {
            Some(mut log) => log.push(message),
            None => eme::log::info!("{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn body() -> PhysicsBody {
        PhysicsBody {
            dynamic: true,
            half_extents: Vec3::splat(0.5),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            grounded: true,
        }
    }

    #[test]
    fn transform_ops_write_the_component() {
        let mut world = World::new();
        let id = world.spawn(Transform::default()).id();
        let missing = world.spawn_empty().id();
        world.despawn(missing);
        {
            let mut ctx = GameContext::new(&mut world);
            assert!(ctx.translate(id, [1.0, 2.0, 0.0]));
            assert_eq!(ctx.translation(id), Some([1.0, 2.0, 0.0]));
            assert!(ctx.rotate_y(id, core::f32::consts::FRAC_PI_2));
            let rotation = ctx.rotation(id).unwrap();
            assert!((rotation[1] - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
            assert!((rotation[3] - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
            assert!(ctx.set_rotation(id, [0.0, 0.0, 0.0, 1.0]));
            assert_eq!(ctx.rotation(id), Some([0.0, 0.0, 0.0, 1.0]));
            assert!(!ctx.set_translation(missing, [0.0; 3]));
            assert!(!ctx.rotate_y(missing, 1.0));
        }
    }

    #[test]
    fn physics_ops_reach_the_body() {
        let mut world = World::new();
        let id = world.spawn((Transform::default(), body())).id();
        {
            let mut ctx = GameContext::new(&mut world);
            assert!(ctx.grounded(id));
            assert_eq!(ctx.velocity(id), Some([0.0; 3]));
            assert!(ctx.set_velocity(id, [0.0, 2.0, 0.0]));
            assert_eq!(ctx.velocity(id), Some([0.0, 2.0, 0.0]));
            assert!(!ctx.grounded(id));
            assert!(ctx.set_angular_velocity(id, [0.0, 1.0, 0.0]));
            assert_eq!(ctx.angular_velocity(id), Some([0.0, 1.0, 0.0]));
            assert!(ctx.add_spin(id, [0.0, 1.0, 0.0]));
            assert!(ctx.angular_velocity(id).unwrap()[1] > 1.0);
        }
    }

    #[test]
    fn controller_ops_reach_the_controller() {
        let mut world = World::new();
        let id = world
            .spawn((
                Transform::default(),
                KinematicController::new(Vec3::splat(0.5)),
            ))
            .id();
        {
            let mut ctx = GameContext::new(&mut world);
            assert!(ctx.walk(id, 2.0, 0.0));
            assert!(ctx.jump(id));
            assert!(!ctx.grounded(id));
        }
        let controller = world.get::<KinematicController>(id).unwrap();
        assert_eq!(controller.desired, Vec3::X);
        assert!(controller.jump);
    }

    #[test]
    fn animation_ops_report_missing_clips() {
        let mut world = World::new();
        let set = AnimationSet {
            clips: vec!["idle".to_owned()],
            ..Default::default()
        };
        let id = world.spawn(set).id();
        {
            let mut ctx = GameContext::new(&mut world);
            assert_eq!(ctx.animation_clips(id).unwrap(), ["idle".to_owned()]);
            assert!(!ctx.play_animation(id, "run"));
            assert!(ctx.play_animation(id, "idle"));
            assert!(ctx.stop_animation(id));
        }
        assert_eq!(
            world.get::<AnimationSet>(id).unwrap().selected.as_deref(),
            Some("idle")
        );
    }

    #[test]
    fn nav_ops_plan_and_follow() {
        let mut world = World::new();
        world.insert_resource(NavGrid::new(Vec2::splat(-5.0), 1.0, 10, 10));
        let id = world.spawn(Transform::default()).id();
        {
            let mut ctx = GameContext::new(&mut world);
            let path = ctx.find_path([-4.0, 0.0, -4.0], [4.0, 0.0, 4.0]).unwrap();
            assert!(path.len() > 1);
            assert!(ctx
                .find_path([100.0, 0.0, 100.0], [4.0, 0.0, 4.0])
                .is_none());
            assert!(ctx.move_to(id, [4.0, 0.0, 4.0], 2.0));
            assert!(ctx.stop_moving(id));
        }
        let agent = world.get::<NavAgent>(id).unwrap();
        assert_eq!(agent.speed, 2.0);
        assert_eq!(agent.goal, None);
    }

    #[test]
    fn input_reads_the_frame() {
        let mut world = World::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyA);
        keys.press(KeyCode::Space);
        world.insert_resource(keys);
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        world.insert_resource(mouse);
        let mut actions = InputActions::default();
        actions.map.insert("jump".to_owned(), vec![KeyCode::Space]);
        world.insert_resource(actions);
        let ctx = GameContext::new(&mut world);
        assert!(ctx.key("a"));
        assert!(ctx.key("A"));
        assert!(ctx.key_down("a"));
        assert!(!ctx.key("b"));
        assert!(ctx.mouse(1));
        assert!(!ctx.mouse(2));
        assert!(!ctx.mouse(9));
        assert!(ctx.action("jump"));
        assert!(ctx.action_down("jump"));
        assert!(!ctx.action("fire"));
    }

    #[test]
    fn time_reports_the_frame() {
        let mut world = World::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(0.25));
        world.insert_resource(time);
        let ctx = GameContext::new(&mut world);
        assert!((ctx.delta() - 0.25).abs() < 1e-6);
        assert!((ctx.elapsed() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn ui_and_scene_ops_fill_their_resources() {
        let mut world = World::new();
        world.insert_resource(ScriptRuntime::default());
        world.insert_resource(ScriptUi::default());
        world.insert_resource(SceneRequests::default());
        let layer;
        {
            let mut ctx = GameContext::new(&mut world);
            ctx.ui_text(0.1, 0.2, 16.0, "score", [1.0, 1.0, 1.0]);
            ctx.ui_panel([0.0, 0.0, 0.5, 0.5], [0.1, 0.2, 0.3, 1.0]);
            ctx.ui_button(7, [0.0, 0.8, 0.2, 0.1], "Start");
            assert!(!ctx.ui_clicked(7));
            assert!(ctx.load_scene("levels/one.ron"));
            layer = ctx.load_scene_additive("levels/overlay.ron").unwrap();
            assert!(ctx.unload_scene(layer));
        }
        assert_eq!(world.resource::<ScriptRuntime>().ui.len(), 3);
        let requests = world.resource::<SceneRequests>();
        assert_eq!(requests.load.as_deref(), Some("levels/one.ron"));
        assert_eq!(
            requests.additive,
            [(layer, "levels/overlay.ron".to_owned())]
        );
        assert_eq!(requests.unload, [layer]);
    }

    #[test]
    fn audio_reports_missing_assets() {
        let dir = std::env::temp_dir().join(format!("fabr-sdk-audio-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sound")).unwrap();
        std::fs::write(dir.join("sound/hit.wav"), b"x").unwrap();
        let mut world = World::new();
        world.insert_resource(ScriptRoot(dir));
        world.insert_resource(ScriptAudio::default());
        world.insert_resource(ScriptLog::default());
        {
            let mut ctx = GameContext::new(&mut world);
            assert!(ctx.play_sound("sound/hit.wav", 2.0, AudioBus::Sfx));
            assert!(!ctx.play_sound("sound/miss.wav", 1.0, AudioBus::Music));
            ctx.log("from rust");
        }
        let requests = &world.resource::<ScriptAudio>().requests;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].volume, 2.0);
        assert_eq!(
            world.resource::<ScriptLog>().lines(),
            ["script audio: missing asset sound/miss.wav", "from rust"]
        );
    }

    #[test]
    fn joints_clamp_their_parameters() {
        let mut world = World::new();
        let a = world.spawn(Transform::default()).id();
        let b = world.spawn(Transform::default()).id();
        let missing = world.spawn_empty().id();
        world.despawn(missing);
        {
            let mut ctx = GameContext::new(&mut world);
            assert!(ctx.create_joint(a, b, 0.0, 900.0, -1.0));
            assert!(!ctx.create_joint(a, missing, 1.0, 1.0, 1.0));
        }
        let joint = world.get::<DistanceJoint>(a).unwrap();
        assert_eq!(joint.rest, 0.05);
        assert_eq!(joint.stiffness, 200.0);
        assert_eq!(joint.damping, 0.0);
        assert_eq!(joint.b, b);
    }

    #[test]
    fn names_resolve_to_entities() {
        let mut world = World::new();
        world.spawn((Name::new("Cube"), Transform::default()));
        let ctx = GameContext::new(&mut world);
        let cube = ctx.find("Cube").expect("named object");
        assert_eq!(ctx.name(cube), Some("Cube"));
        assert!(ctx.find("Missing").is_none());
    }
}
