//! Lua gameplay scripting shared by the Fabr editor and the runtime player.
//!
//! A script returns a module table with optional `on_start`, `on_update` and
//! `on_stop` functions plus a `properties` table. The host builds an isolated
//! environment per attached object and exposes a small `fabr` API around the
//! ECS. Scripts only run while [`ScriptsActive`] is true.

extern crate alloc;

use alloc::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use alloc::sync::Arc;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use eme_app::{App, Plugin, Update};
use eme_asset::{AssetServer, Handle};
use eme_audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use eme_ecs::prelude::*;
use eme_ecs::system::SystemParam;
use eme_anim::AnimationSet;
use eme_game_runtime::{
    DistanceJoint, KinematicController, NavAgent, NavGrid, PhysicsBody,
};
use eme_game_scene::{ScriptLanguage, ScriptRef, ScriptValue};
use eme_input::prelude::*;
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;
use mlua::{Lua, RegistryKey, Table, Value};

mod csharp;

pub use csharp::CSharpScripts;

/// Marks a spawned entity that runs a Lua script.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ScriptedObject {
    /// Scene object id, stable for the lifetime of the document.
    pub object_id: u64,
    /// Script path and author property overrides.
    pub script: ScriptRef,
}

/// Whether scripts may run. The editor clears this while not in Game view.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ScriptsActive(pub bool);

/// Folder scripts are resolved against; the editor sets its asset root.
#[derive(Resource, Clone, Debug)]
pub struct ScriptRoot(pub PathBuf);

impl Default for ScriptRoot {
    fn default() -> Self {
        Self(PathBuf::from("assets"))
    }
}

/// Lines emitted by `fabr.log` and by script failures.
#[derive(Resource, Default, Debug)]
pub struct ScriptLog {
    lines: Vec<String>,
    rev: u64,
}

impl ScriptLog {
    /// Appends one line.
    pub fn push(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
        self.rev = self.rev.wrapping_add(1);
    }

    /// Lines in order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Bumped on every line.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Takes the lines, leaving the log empty.
    pub fn drain(&mut self) -> Vec<String> {
        self.rev = self.rev.wrapping_add(1);
        std::mem::take(&mut self.lines)
    }
}

/// Named input actions shared by scripts and gameplay code.
#[derive(Resource, Clone, Debug, Default)]
pub struct InputActions {
    /// Action name to parsed key bindings.
    pub map: BTreeMap<String, Vec<KeyCode>>,
}

impl InputActions {
    /// Builds an action map from key names, rejecting unknown key names.
    pub fn from_names(names: &BTreeMap<String, Vec<String>>) -> Result<Self, String> {
        let mut map = BTreeMap::new();
        for (action, keys) in names {
            let mut parsed = Vec::new();
            for key in keys {
                let Some(code) = key_binding(key) else {
                    return Err(format!("unknown key `{key}` in action `{action}`"));
                };
                parsed.push(code);
            }
            map.insert(action.clone(), parsed);
        }
        Ok(Self { map })
    }
}

/// Parses a display key name used by project settings and scripts.
pub fn key_binding(name: &str) -> Option<KeyCode> {
    match name.trim().to_ascii_lowercase().as_str() {
        "space" => Some(KeyCode::Space),
        "enter" => Some(KeyCode::Enter),
        "escape" => Some(KeyCode::Escape),
        "shift" | "shiftleft" => Some(KeyCode::ShiftLeft),
        "control" | "ctrl" | "controlleft" => Some(KeyCode::ControlLeft),
        "up" | "arrowup" => Some(KeyCode::ArrowUp),
        "down" | "arrowdown" => Some(KeyCode::ArrowDown),
        "left" | "arrowleft" => Some(KeyCode::ArrowLeft),
        "right" | "arrowright" => Some(KeyCode::ArrowRight),
        "a" => Some(KeyCode::KeyA),
        "b" => Some(KeyCode::KeyB),
        "c" => Some(KeyCode::KeyC),
        "d" => Some(KeyCode::KeyD),
        "e" => Some(KeyCode::KeyE),
        "f" => Some(KeyCode::KeyF),
        "g" => Some(KeyCode::KeyG),
        "h" => Some(KeyCode::KeyH),
        "i" => Some(KeyCode::KeyI),
        "j" => Some(KeyCode::KeyJ),
        "k" => Some(KeyCode::KeyK),
        "l" => Some(KeyCode::KeyL),
        "m" => Some(KeyCode::KeyM),
        "n" => Some(KeyCode::KeyN),
        "o" => Some(KeyCode::KeyO),
        "p" => Some(KeyCode::KeyP),
        "q" => Some(KeyCode::KeyQ),
        "r" => Some(KeyCode::KeyR),
        "s" => Some(KeyCode::KeyS),
        "t" => Some(KeyCode::KeyT),
        "u" => Some(KeyCode::KeyU),
        "v" => Some(KeyCode::KeyV),
        "w" => Some(KeyCode::KeyW),
        "x" => Some(KeyCode::KeyX),
        "y" => Some(KeyCode::KeyY),
        "z" => Some(KeyCode::KeyZ),
        "0" => Some(KeyCode::Digit0),
        "1" => Some(KeyCode::Digit1),
        "2" => Some(KeyCode::Digit2),
        "3" => Some(KeyCode::Digit3),
        "4" => Some(KeyCode::Digit4),
        "5" => Some(KeyCode::Digit5),
        "6" => Some(KeyCode::Digit6),
        "7" => Some(KeyCode::Digit7),
        "8" => Some(KeyCode::Digit8),
        "9" => Some(KeyCode::Digit9),
        _ => None,
    }
}

/// Bus a script sound is routed through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AudioBus {
    /// Sound effects, the default.
    #[default]
    Sfx,
    /// Music.
    Music,
}

impl AudioBus {
    /// Stable label used by logs and the mixer.
    pub fn label(self) -> &'static str {
        match self {
            AudioBus::Sfx => "sfx",
            AudioBus::Music => "music",
        }
    }

    /// Parses the optional Lua bus name.
    pub fn parse(name: &str) -> Self {
        if name.trim().eq_ignore_ascii_case("music") {
            AudioBus::Music
        } else {
            AudioBus::Sfx
        }
    }
}

/// One clip a script asked to play, relative to [`ScriptRoot`].
#[derive(Clone, Debug, PartialEq)]
pub struct AudioRequest {
    /// Asset path relative to the asset root.
    pub path: String,
    /// Linear volume from 0 to 4.
    pub volume: f32,
    /// Bus this request is routed through.
    pub bus: AudioBus,
}

/// Per-bus gains supplied by project settings.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct AudioMix {
    /// Sound-effect bus gain.
    pub sfx: f32,
    /// Music bus gain.
    pub music: f32,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self {
            sfx: 1.0,
            music: 1.0,
        }
    }
}

impl AudioMix {
    /// Gain for one bus.
    pub fn gain(self, bus: AudioBus) -> f32 {
        match bus {
            AudioBus::Sfx => self.sfx,
            AudioBus::Music => self.music,
        }
    }
}

fn effective_volume(request: &AudioRequest, mix: &AudioMix) -> f32 {
    (request.volume * mix.gain(request.bus)).clamp(0.0, 4.0)
}

/// Script audio requests waiting for an [`AssetServer`] to turn into players.
#[derive(Resource, Default, Debug)]
pub struct ScriptAudio {
    /// Requests queued by the last script update.
    pub requests: Vec<AudioRequest>,
}

/// One HUD element drawn by a script.
#[derive(Clone, Debug, PartialEq)]
pub enum UiElement {
    /// Filled rectangle, normalised coordinates.
    Panel {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Linear RGBA.
        color: [f32; 4],
    },
    /// Text label.
    Text {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Font size in pixels.
        size: f32,
        /// Text to draw.
        text: String,
        /// Linear RGB.
        color: [f32; 3],
    },
    /// Clickable labelled rectangle.
    Button {
        /// Identifier reported by `fabr.ui.clicked`.
        id: u32,
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
        /// Button label.
        label: String,
    },
}

/// HUD drawn by scripts.
#[derive(Resource, Clone, Default, Debug)]
pub struct ScriptUi {
    /// Elements in draw order.
    pub elements: Vec<UiElement>,
    /// Button ids pressed since the last script update.
    pub clicks: Vec<u32>,
}

impl ScriptUi {
    /// Removes every element.
    pub fn clear(&mut self) { self.elements.clear(); }
    /// Topmost button under a normalised point.
    pub fn button_at(&self, x: f32, y: f32) -> Option<u32> {
        self.elements.iter().rev().find_map(|e| match e {
            UiElement::Button { id, x: bx, y: by, w, h, .. }
                if x >= *bx && x <= bx + w && y >= *by && y <= by + h => Some(*id),
            _ => None,
        })
    }
    /// Whether a button was pressed since the last update.
    pub fn clicked(&self, id: u32) -> bool { self.clicks.contains(&id) }
}

/// Runtime scene-load requests raised by scripts.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct SceneRequests {
    /// Scene file to load, replacing the current one.
    pub load: Option<String>,
    /// Reload the current scene file.
    pub reload: bool,
    /// Extra scene documents to add, with the layer id the script received.
    pub additive: Vec<(u64, String)>,
    /// Layer ids requested for removal.
    pub unload: Vec<u64>,
}

impl SceneRequests {
    /// Whether a request is waiting.
    pub fn pending(&self) -> bool {
        self.load.is_some()
            || self.reload
            || !self.additive.is_empty()
            || !self.unload.is_empty()
    }
}

/// Paths (relative to [`ScriptRoot`]) the editor wants reloaded.
#[derive(Resource, Default, Debug)]
pub struct ScriptReload {
    /// Pending script paths.
    pub paths: Vec<String>,
}

impl ScriptReload {
    /// Queues one path for the next sync pass.
    pub fn request(&mut self, path: impl Into<String>) {
        self.paths.push(path.into());
    }
}

struct Instance {
    object_id: u64,
    path: String,
    module: RegistryKey,
    self_table: RegistryKey,
    api: Arc<Mutex<ApiState>>,
    started: bool,
    failed: bool,
}

#[derive(Clone, Debug)]
enum AnimationOp {
    Play(String),
    Stop,
}

#[derive(Clone, Debug)]
enum ControllerOp {
    Walk(f32, f32),
    Jump,
}

/// One scene operation raised by a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneOp {
    /// Replace the running document with `path`.
    Load(String),
    /// Add `path` as a layer with the id returned to the script.
    Additive {
        /// Layer id handed to the caller.
        id: u64,
        /// Scene file to add.
        path: String,
    },
    /// Remove the layer with this id.
    Unload(u64),
    /// Re-read the running document.
    Reload,
}

/// Ids for additive scene layers, unique for the process lifetime.
static NEXT_LAYER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Allocates an id for an additive scene layer.
pub fn next_layer_id() -> u64 {
    NEXT_LAYER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Debug)]
enum NavOp {
    Go { goal: Vec3, speed: f32 },
    Stop,
}

/// Read-only world view shared by every script instance in a frame.
///
/// The scripting pass builds one of these per update and hands each instance an
/// `Arc`; scripts never see a per-instance copy of the scene.
#[derive(Default)]
struct FrameState {
    transforms: HashMap<Entity, (Vec3, Quat)>,
    names: HashMap<String, Entity>,
    keys: HashSet<String>,
    keys_down: HashSet<String>,
    actions: HashMap<String, bool>,
    actions_down: HashMap<String, bool>,
    mouse: HashSet<u8>,
    velocities: HashMap<Entity, Vec3>,
    angular: HashMap<Entity, Vec3>,
    grounded: HashSet<Entity>,
    anim_clips: HashMap<Entity, Vec<String>>,
    nav: Option<Arc<NavGrid>>,
    ui_clicks: Vec<u32>,
    delta: f32,
    elapsed: f32,
}

/// Writes a script makes to its own view before the frame ends.
#[derive(Default)]
struct LocalFrame {
    transforms: HashMap<Entity, (Vec3, Quat)>,
    velocities: HashMap<Entity, Vec3>,
    angular: HashMap<Entity, Vec3>,
    grounded: HashMap<Entity, bool>,
}

struct ApiState {
    self_entity: Entity,
    frame: Arc<FrameState>,
    local: LocalFrame,
    ops: Vec<(Entity, Vec3, Quat)>,
    log: Vec<String>,
    audio: Vec<AudioRequest>,
    velocity_ops: Vec<(Entity, Vec3)>,
    angular_ops: Vec<(Entity, Vec3)>,
    spin_ops: Vec<(Entity, Vec3)>,
    joint_ops: Vec<(Entity, Entity, f32, f32, f32)>,
    animation_ops: Vec<(Entity, AnimationOp)>,
    nav_ops: Vec<(Entity, NavOp)>,
    scene_ops: Vec<SceneOp>,
    ui: Vec<UiElement>,
    controller_ops: Vec<(Entity, ControllerOp)>,
    properties: BTreeMap<String, ScriptValue>,
}

impl Default for ApiState {
    fn default() -> Self {
        Self {
            self_entity: Entity::PLACEHOLDER,
            frame: Arc::new(FrameState::default()),
            local: LocalFrame::default(),
            ops: Vec::new(),
            log: Vec::new(),
            audio: Vec::new(),
            velocity_ops: Vec::new(),
            angular_ops: Vec::new(),
            spin_ops: Vec::new(),
            joint_ops: Vec::new(),
            animation_ops: Vec::new(),
            nav_ops: Vec::new(),
            scene_ops: Vec::new(),
            ui: Vec::new(),
            controller_ops: Vec::new(),
            properties: BTreeMap::new(),
        }
    }
}

impl ApiState {
    fn transform(&self, entity: Entity) -> (Vec3, Quat) {
        self.local
            .transforms
            .get(&entity)
            .copied()
            .or_else(|| self.frame.transforms.get(&entity).copied())
            .unwrap_or_default()
    }

    fn set_transform(&mut self, entity: Entity, translation: Vec3, rotation: Quat) {
        self.local
            .transforms
            .insert(entity, (translation, rotation));
    }

    fn velocity(&self, entity: Entity) -> Vec3 {
        self.local
            .velocities
            .get(&entity)
            .copied()
            .or_else(|| self.frame.velocities.get(&entity).copied())
            .unwrap_or_default()
    }

    fn grounded(&self, entity: Entity) -> bool {
        self.local
            .grounded
            .get(&entity)
            .copied()
            .unwrap_or_else(|| self.frame.grounded.contains(&entity))
    }

    fn angular_velocity(&self, entity: Entity) -> Vec3 {
        self.local
            .angular
            .get(&entity)
            .copied()
            .or_else(|| self.frame.angular.get(&entity).copied())
            .unwrap_or_default()
    }

    fn reset_frame(&mut self, frame: Arc<FrameState>) {
        self.frame = frame;
        self.local.transforms.clear();
        self.local.velocities.clear();
        self.local.angular.clear();
        self.local.grounded.clear();
        self.ops.clear();
        self.log.clear();
        self.audio.clear();
        self.velocity_ops.clear();
        self.angular_ops.clear();
        self.spin_ops.clear();
        self.joint_ops.clear();
        self.animation_ops.clear();
        self.nav_ops.clear();
        self.scene_ops.clear();
        self.ui.clear();
        self.controller_ops.clear();
    }
}

impl ApiState {
    fn translation(&self, entity: Entity) -> Vec3 {
        self.transform(entity).0
    }

    fn rotation(&self, entity: Entity) -> Quat {
        self.transform(entity).1
    }

    fn set_translation(&mut self, entity: Entity, translation: Vec3) {
        let (_, rotation) = self.transform(entity);
        self.set_transform(entity, translation, rotation);
        self.ops.retain(|(target, _, _)| *target != entity);
        self.ops.push((entity, translation, rotation));
    }

    fn translate(&mut self, entity: Entity, delta: Vec3) {
        self.set_translation(entity, self.translation(entity) + delta);
    }

    fn set_rotation(&mut self, entity: Entity, rotation: Quat) {
        let (translation, _) = self.transform(entity);
        self.set_transform(entity, translation, rotation);
        self.ops.retain(|(target, _, _)| *target != entity);
        self.ops.push((entity, translation, rotation));
    }

    fn rotate_y(&mut self, entity: Entity, radians: f32) {
        self.set_rotation(entity, Quat::from_rotation_y(radians) * self.rotation(entity));
    }

    fn set_velocity(&mut self, entity: Entity, velocity: Vec3) {
        self.local.velocities.insert(entity, velocity);
        self.local.grounded.insert(entity, false);
        self.velocity_ops.retain(|(target, _)| *target != entity);
        self.velocity_ops.push((entity, velocity));
    }

    fn set_angular_velocity(&mut self, entity: Entity, angular: Vec3) {
        self.local.angular.insert(entity, angular);
        self.angular_ops.push((entity, angular));
    }

    fn add_spin(&mut self, entity: Entity, impulse: Vec3) {
        self.spin_ops.push((entity, impulse));
    }

    fn walk(&mut self, entity: Entity, x: f32, z: f32) {
        self.controller_ops.push((entity, ControllerOp::Walk(x, z)));
    }

    fn jump(&mut self, entity: Entity) {
        self.controller_ops.push((entity, ControllerOp::Jump));
    }

    fn animation_clips(&self, entity: Entity) -> &[String] {
        self.frame.anim_clips.get(&entity).map_or(&[], Vec::as_slice)
    }

    fn play_animation(&mut self, entity: Entity, name: String) -> bool {
        if !self.animation_clips(entity).iter().any(|clip| clip == &name) {
            self.log.push(format!("animation: missing clip {name}"));
            return false;
        }
        self.animation_ops.push((entity, AnimationOp::Play(name)));
        true
    }

    fn stop_animation(&mut self, entity: Entity) {
        self.animation_ops.push((entity, AnimationOp::Stop));
    }

    fn find_path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        self.frame.nav.as_ref()?.find_path(from, to)
    }

    fn move_to(&mut self, entity: Entity, goal: Vec3, speed: f32) {
        let speed = speed.clamp(0.1, 50.0);
        self.nav_ops.push((entity, NavOp::Go { goal, speed }));
    }

    fn stop_moving(&mut self, entity: Entity) {
        self.nav_ops.push((entity, NavOp::Stop));
    }

    fn play_sound(&mut self, path: String, volume: f32, bus: AudioBus) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 4.0)
        } else {
            1.0
        };
        self.audio.push(AudioRequest { path, volume, bus });
    }

    fn ui_text(&mut self, x: f32, y: f32, size: f32, text: String, color: [f32; 3]) {
        self.ui.push(UiElement::Text {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            size: size.clamp(4.0, 96.0),
            text,
            color,
        });
    }

    fn ui_panel(&mut self, rect: [f32; 4], color: [f32; 4]) {
        self.ui.push(UiElement::Panel {
            x: rect[0].clamp(0.0, 1.0),
            y: rect[1].clamp(0.0, 1.0),
            w: rect[2].clamp(0.0, 1.0),
            h: rect[3].clamp(0.0, 1.0),
            color,
        });
    }

    fn ui_button(&mut self, id: u32, rect: [f32; 4], label: String) {
        self.ui.push(UiElement::Button {
            id,
            x: rect[0].clamp(0.0, 1.0),
            y: rect[1].clamp(0.0, 1.0),
            w: rect[2].clamp(0.0, 1.0),
            h: rect[3].clamp(0.0, 1.0),
            label,
        });
    }

    fn load_scene(&mut self, path: String) {
        self.scene_ops.push(SceneOp::Load(path));
    }

    fn reload_scene(&mut self) {
        self.scene_ops.push(SceneOp::Reload);
    }

    fn load_scene_additive(&mut self, path: String) -> u64 {
        let id = next_layer_id();
        self.scene_ops.push(SceneOp::Additive { id, path });
        id
    }

    fn unload_scene(&mut self, id: u64) {
        self.scene_ops.push(SceneOp::Unload(id));
    }

    fn create_joint(&mut self, a: Entity, b: Entity, rest: f32, stiffness: f32, damping: f32) {
        self.joint_ops.push((a, b, rest, stiffness, damping));
    }

    fn find(&self, name: &str) -> Option<Entity> {
        self.frame.names.get(name).copied()
    }
}

/// Lua VM plus one instance per scripted entity.
#[derive(Resource)]
pub struct ScriptRuntime {
    /// Scene requests raised by scripts, drained by [`publish_scene_requests`].
    pub scene_requests: Vec<SceneOp>,
    /// HUD elements collected this update.
    pub ui: Vec<UiElement>,
    /// Button ids pressed since the last script update.
    pub ui_clicks: Vec<u32>,
    lua: Lua,
    instances: HashMap<Entity, Instance>,
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self {
            scene_requests: Vec::new(),
            ui: Vec::new(),
            ui_clicks: Vec::new(),
            lua: Lua::new(),
            instances: HashMap::new(),
        }
    }
}

/// The frame [`update_scripts`] built, shared with the C# pass.
#[derive(Resource, Clone, Default)]
pub struct ScriptFrame(Arc<FrameState>);

impl ScriptRuntime {
    /// Evaluates a script module in a fresh environment.
    fn eval_script(
        lua: &Lua,
        path: &Path,
        src: &str,
        fabr: Option<Table>,
    ) -> mlua::Result<Table> {
        let env = lua.create_table()?;
        let meta = lua.create_table()?;
        meta.set("__index", lua.globals())?;
        env.set_metatable(Some(meta));
        if let Some(fabr) = fabr {
            env.set("fabr", fabr)?;
        }
        lua.load(src)
            .set_name(path.to_string_lossy())
            .set_environment(env)
            .eval()
    }

    fn build_api(
        lua: &Lua,
        entity: Entity,
        name: &str,
        properties: BTreeMap<String, ScriptValue>,
    ) -> mlua::Result<(Arc<Mutex<ApiState>>, Table)> {
        let api = Arc::new(Mutex::new(ApiState {
            self_entity: entity,
            properties,
            ..Default::default()
        }));
        let self_table = lua.create_table()?;
        self_table.set("entity", entity.to_bits() as i64)?;
        self_table.set("name", name)?;
        self_table.set("properties", properties_table(lua, &api)?)?;

        let api_get = api.clone();
        self_table.set("get_translation", lua.create_function(move |_, _self: Value| {
            let state = api_get.lock().unwrap();
            let t = state.translation(state.self_entity);
            Ok((t.x, t.y, t.z))
        })?)?;
        let api_set = api.clone();
        self_table.set("set_translation", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
            let mut state = api_set.lock().unwrap();
            let entity = state.self_entity;
            state.set_translation(entity, Vec3::new(x, y, z));
            Ok(())
        })?)?;
        let api_move = api.clone();
        self_table.set("translate", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
            let mut state = api_move.lock().unwrap();
            let entity = state.self_entity;
            state.translate(entity, Vec3::new(x, y, z));
            Ok(())
        })?)?;
        let api_rotate = api.clone();
        self_table.set("rotate_y", lua.create_function(move |_, (_self, radians): (Value, f32)| {
            let mut state = api_rotate.lock().unwrap();
            let entity = state.self_entity;
            state.rotate_y(entity, radians);
            Ok(())
        })?)?;

        let api_vel = api.clone();
        self_table.set("get_velocity", lua.create_function(move |_, _self: Value| {
            let state = api_vel.lock().unwrap();
            let velocity = state.velocity(state.self_entity);
            Ok((velocity.x, velocity.y, velocity.z))
        })?)?;
        let api_grounded = api.clone();
        self_table.set("is_grounded", lua.create_function(move |_, _self: Value| {
            let state = api_grounded.lock().unwrap();
            Ok(state.grounded(state.self_entity))
        })?)?;
        let api_set_vel = api.clone();
        self_table.set(
            "set_velocity",
            lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
                let mut state = api_set_vel.lock().unwrap();
                let entity = state.self_entity;
                state.set_velocity(entity, Vec3::new(x, y, z));
                Ok(())
            })?,
        )?;

        let api_set_spin = api.clone();
        self_table.set(
            "set_angular_velocity",
            lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
                let mut state = api_set_spin.lock().unwrap();
                let entity = state.self_entity;
                state.set_angular_velocity(entity, Vec3::new(x, y, z));
                Ok(())
            })?,
        )?;
        let api_get_spin = api.clone();
        self_table.set(
            "get_angular_velocity",
            lua.create_function(move |_, _self: Value| {
                let state = api_get_spin.lock().unwrap();
                let spin = state.angular_velocity(state.self_entity);
                Ok((spin.x, spin.y, spin.z))
            })?,
        )?;
        let api_add_spin = api.clone();
        self_table.set(
            "add_spin",
            lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
                let mut state = api_add_spin.lock().unwrap();
                let entity = state.self_entity;
                state.add_spin(entity, Vec3::new(x, y, z));
                Ok(())
            })?,
        )?;

        let api_clips = api.clone();
        self_table.set(
            "animation_clips",
            lua.create_function(move |lua, _self: Value| {
                let state = api_clips.lock().unwrap();
                let table = lua.create_table()?;
                for (index, clip) in state.animation_clips(state.self_entity).iter().enumerate() {
                    table.set(index + 1, clip.clone())?;
                }
                Ok(table)
            })?,
        )?;
        let api_play_anim = api.clone();
        self_table.set(
            "play_animation",
            lua.create_function(move |_, (_self, name): (Value, String)| {
                let mut state = api_play_anim.lock().unwrap();
                let entity = state.self_entity;
                Ok(state.play_animation(entity, name))
            })?,
        )?;
        let api_stop_anim = api.clone();
        self_table.set(
            "stop_animation",
            lua.create_function(move |_, _self: Value| {
                let mut state = api_stop_anim.lock().unwrap();
                let entity = state.self_entity;
                state.stop_animation(entity);
                Ok(())
            })?,
        )?;

        let api_move = api.clone();
        self_table.set(
            "move_to",
            lua.create_function(
                move |_, (_self, x, y, z, speed): (Value, f32, f32, f32, Option<f32>)| {
                    let speed = speed.filter(|speed| speed.is_finite()).unwrap_or(3.0);
                    let mut state = api_move.lock().unwrap();
                    let entity = state.self_entity;
                    state.move_to(entity, Vec3::new(x, y, z), speed);
                    Ok(())
                },
            )?,
        )?;
        let api_stop_move = api.clone();
        self_table.set(
            "stop_moving",
            lua.create_function(move |_, _self: Value| {
                let mut state = api_stop_move.lock().unwrap();
                let entity = state.self_entity;
                state.stop_moving(entity);
                Ok(())
            })?,
        )?;

        let fabr = lua.create_table()?;
        let api_log = api.clone();
        fabr.set("log", lua.create_function(move |_, value: Value| {
            let text = match value {
                Value::String(s) => s.to_str()?.to_owned(),
                Value::Integer(n) => n.to_string(),
                Value::Number(n) => n.to_string(),
                Value::Boolean(b) => b.to_string(),
                Value::Nil => "nil".to_string(),
                other => format!("{other:?}"),
            };
            api_log.lock().unwrap().log.push(text);
            Ok(())
        })?)?;
        let api_delta = api.clone();
        fabr.set("time", lua.create_table_with_capacity(0, 2)?)?;
        if let Ok(table) = fabr.get::<Table>("time") {
            let api = api_delta.clone();
            table.set("delta", lua.create_function(move |_, ()| Ok(api.lock().unwrap().frame.delta))?)?;
            let api = api_delta;
            table.set("elapsed", lua.create_function(move |_, ()| Ok(api.lock().unwrap().frame.elapsed))?)?;
        }
        let api_key = api.clone();
        let api_down = api.clone();
        let api_mouse = api.clone();
        let input = lua.create_table()?;
        let api_play = api.clone();
        let audio = lua.create_table()?;
        audio.set(
            "play",
            lua.create_function(
                move |_, (path, volume, bus): (String, Option<f32>, Option<String>)| {
                    let bus = bus.as_deref().map(AudioBus::parse).unwrap_or_default();
                    api_play
                        .lock()
                        .unwrap()
                        .play_sound(path, volume.unwrap_or(1.0), bus);
                    Ok(())
                },
            )?,
        )?;
        input.set("key", lua.create_function(move |_, name: String| {
            Ok(api_key.lock().unwrap().frame.keys.contains(&normalize_key(&name)))
        })?)?;
        input.set("key_down", lua.create_function(move |_, name: String| {
            Ok(api_down.lock().unwrap().frame.keys_down.contains(&normalize_key(&name)))
        })?)?;
        input.set("mouse_pressed", lua.create_function(move |_, button: u8| {
            Ok(api_mouse.lock().unwrap().frame.mouse.contains(&button))
        })?)?;
        let api_action = api.clone();
        input.set("action", lua.create_function(move |_, name: String| {
            Ok(api_action.lock().unwrap().frame.actions.get(&name).copied().unwrap_or(false))
        })?)?;
        let api_action_down = api.clone();
        input.set("action_down", lua.create_function(move |_, name: String| {
            Ok(api_action_down
                .lock()
                .unwrap()
                .frame
                .actions_down
                .get(&name)
                .copied()
                .unwrap_or(false))
        })?)?;
        fabr.set("input", input)?;
        fabr.set("audio", audio)?;
        let api_nav = api.clone();
        let nav = lua.create_table()?;
        nav.set(
            "find_path",
            lua.create_function(
                move |lua, (x1, y1, z1, x2, y2, z2): (f32, f32, f32, f32, f32, f32)| {
                    let state = api_nav.lock().unwrap();
                    let table = lua.create_table()?;
                    let Some(path) = state.find_path(
                        Vec3::new(x1, y1, z1),
                        Vec3::new(x2, y2, z2),
                    ) else {
                        return Ok(table);
                    };
                    for (index, point) in path.iter().enumerate() {
                        let entry = lua.create_table()?;
                        entry.set("x", point.x)?;
                        entry.set("y", point.y)?;
                        entry.set("z", point.z)?;
                        table.set(index + 1, entry)?;
                    }
                    Ok(table)
                },
            )?,
        )?;
        fabr.set("nav", nav)?;
        let api_ui = api.clone();
        let ui = lua.create_table()?;
        ui.set("clear", lua.create_function(move |_, ()| { api_ui.lock().unwrap().ui.clear(); Ok(()) })?)?;
        let api_text = api.clone();
        ui.set("text", lua.create_function(move |_, (x, y, size, text, r, g, b): (f32, f32, f32, String, Option<f32>, Option<f32>, Option<f32>)| {
            api_text.lock().unwrap().ui_text(x, y, size, text, [r.unwrap_or(1.0), g.unwrap_or(1.0), b.unwrap_or(1.0)]);
            Ok(())
        })?)?;
        let api_panel = api.clone();
        ui.set("panel", lua.create_function(move |_, (x, y, w, h, r, g, b, a): (f32, f32, f32, f32, f32, f32, f32, f32)| {
            api_panel.lock().unwrap().ui_panel([x, y, w, h], [r, g, b, a]);
            Ok(())
        })?)?;
        let api_button = api.clone();
        ui.set("button", lua.create_function(move |_, (id, x, y, w, h, label): (u32, f32, f32, f32, f32, String)| {
            api_button.lock().unwrap().ui_button(id, [x, y, w, h], label);
            Ok(())
        })?)?;
        let api_clicked = api.clone();
        ui.set("clicked", lua.create_function(move |_, id: u32| Ok(api_clicked.lock().unwrap().frame.ui_clicks.contains(&id)))?)?;
        fabr.set("ui", ui)?;
        let api_walk = api.clone();
        self_table.set(
            "walk",
            lua.create_function(move |_, (_self, x, z): (Value, f32, f32)| {
                let mut state = api_walk.lock().unwrap();
                let entity = state.self_entity;
                state.walk(entity, x, z);
                Ok(())
            })?,
        )?;
        let api_jump = api.clone();
        self_table.set(
            "jump",
            lua.create_function(move |_, _self: Value| {
                let mut state = api_jump.lock().unwrap();
                let entity = state.self_entity;
                state.jump(entity);
                Ok(())
            })?,
        )?;

        let api_scene = api.clone();
        let scene = lua.create_table()?;
        scene.set(
            "load",
            lua.create_function(move |_, path: String| {
                api_scene.lock().unwrap().load_scene(path);
                Ok(true)
            })?,
        )?;
        let api_reload = api.clone();
        scene.set(
            "reload",
            lua.create_function(move |_, ()| {
                api_reload.lock().unwrap().reload_scene();
                Ok(true)
            })?,
        )?;
        let api_additive = api.clone();
        scene.set(
            "load_additive",
            lua.create_function(move |_, path: String| {
                Ok(api_additive.lock().unwrap().load_scene_additive(path))
            })?,
        )?;
        let api_unload = api.clone();
        scene.set(
            "unload",
            lua.create_function(move |_, id: u64| {
                api_unload.lock().unwrap().unload_scene(id);
                Ok(true)
            })?,
        )?;
        fabr.set("scene", scene)?;
        let api_joint = api.clone();
        let joint = lua.create_table()?;
        joint.set(
            "create",
            lua.create_function(
                move |_, (a, b, rest, stiffness, damping): (String, String, f32, f32, f32)| {
                    let mut state = api_joint.lock().unwrap();
                    let (Some(first), Some(second)) = (state.find(&a), state.find(&b)) else {
                        return Ok(false);
                    };
                    state.create_joint(first, second, rest, stiffness, damping);
                    Ok(true)
                },
            )?,
        )?;
        fabr.set("joint", joint)?;

        let api_find = api.clone();
        fabr.set("entity", lua.create_table()?)?;
        if let Ok(table) = fabr.get::<Table>("entity") {
            table.set("find", lua.create_function(move |lua, name: String| {
                let entity = api_find.lock().unwrap().find(&name);
                match entity {
                    Some(entity) => entity_handle(lua, api_find.clone(), entity, name),
                    None => Ok(Value::Nil),
                }
            })?)?;
        }
        self_table.set("fabr", fabr.clone())?;
        Ok((api, self_table))
    }
}

fn entity_handle(lua: &Lua, api: Arc<Mutex<ApiState>>, entity: Entity, name: String) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    table.set("entity", entity.to_bits() as i64)?;
    table.set("name", name)?;
    let get = api.clone();
    table.set("get_translation", lua.create_function(move |_, _self: Value| {
        let state = get.lock().unwrap();
        let (t, _) = state.transform(entity);
        Ok((t.x, t.y, t.z))
    })?)?;
    let set = api.clone();
    table.set("set_translation", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
        let mut state = set.lock().unwrap();
        let (_, r) = state.transform(entity);
        state.set_transform(entity, Vec3::new(x, y, z), r);
        state.ops.retain(|(e, _, _)| *e != entity);
        state.ops.push((entity, Vec3::new(x, y, z), r));
        Ok(())
    })?)?;
    let translate = api.clone();
    table.set("translate", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
        let mut state = translate.lock().unwrap();
        let (t, r) = state.transform(entity);
        let next = t + Vec3::new(x, y, z);
        state.set_transform(entity, next, r);
        state.ops.retain(|(e, _, _)| *e != entity);
        state.ops.push((entity, next, r));
        Ok(())
    })?)?;
    let rotate = api;
    table.set("rotate_y", lua.create_function(move |_, (_self, radians): (Value, f32)| {
        let mut state = rotate.lock().unwrap();
        let (t, r) = state.transform(entity);
        let next = Quat::from_rotation_y(radians) * r;
        state.set_transform(entity, t, next);
        state.ops.retain(|(e, _, _)| *e != entity);
        state.ops.push((entity, t, next));
        Ok(())
    })?)?;
    Ok(Value::Table(table))
}

fn properties_table(lua: &Lua, api: &Arc<Mutex<ApiState>>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let values = api.lock().unwrap().properties.clone();
    for (key, value) in values {
        set_property(lua, &table, &key, &value)?;
    }
    Ok(table)
}

fn set_property(lua: &Lua, table: &Table, key: &str, value: &ScriptValue) -> mlua::Result<()> {
    match value {
        ScriptValue::Bool(value) => table.set(key, *value),
        ScriptValue::Number(value) => table.set(key, *value),
        ScriptValue::Text(value) => table.set(key, lua.create_string(value)?),
    }
}

fn properties_from(module: &Table) -> mlua::Result<BTreeMap<String, ScriptValue>> {
    let mut out = BTreeMap::new();
    let Ok(properties) = module.get::<Table>("properties") else {
        return Ok(out);
    };
    for pair in properties.pairs::<String, Value>() {
        let (key, value) = pair?;
        out.insert(key, script_value(value)?);
    }
    Ok(out)
}

fn script_value(value: Value) -> mlua::Result<ScriptValue> {
    match value {
        Value::Boolean(value) => Ok(ScriptValue::Bool(value)),
        Value::Integer(value) => Ok(ScriptValue::Number(value as f64)),
        Value::Number(value) => Ok(ScriptValue::Number(value)),
        Value::String(value) => Ok(ScriptValue::Text(value.to_str()?.to_owned())),
        other => Err(mlua::Error::RuntimeError(format!(
            "property must be bool, number or string, got {other:?}"
        ))),
    }
}

fn normalize_key(name: &str) -> String {
    let mut name = name.trim().to_owned();
    if let Some(rest) = name.strip_prefix("Key") {
        name = rest.to_owned();
    } else if let Some(rest) = name.strip_prefix("Digit") {
        name = rest.to_owned();
    }
    name.to_ascii_lowercase()
}

/// Script files under `root/scripts`, relative to `root`.
pub fn script_files(root: &Path) -> Vec<String> {
    script_files_for(root, ScriptLanguage::Lua)
}

/// Script artifacts of one language under `<root>/scripts`, relative to `root`.
pub fn script_files_for(root: &Path, language: ScriptLanguage) -> Vec<String> {
    let mut out = Vec::new();
    collect_scripts(
        &root.join("scripts"),
        root,
        0,
        language.extension(),
        &mut out,
    );
    out.sort();
    out
}

fn collect_scripts(dir: &Path, root: &Path, depth: usize, extension: &str, out: &mut Vec<String>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scripts(&path, root, depth + 1, extension, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
            && !is_framework_assembly(&path)
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Runtime assemblies that sit beside game scripts but are not scripts.
fn is_framework_assembly(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name == csharp::BINDINGS_ASSEMBLY || name.ends_with(".resources.dll")
}

/// Reads the property defaults a script declares, without running its lifecycle.
pub fn script_property_defaults(
    root: &Path,
    path: &str,
) -> Result<BTreeMap<String, ScriptValue>, String> {
    let full = root.join(path);
    let src = std::fs::read_to_string(&full).map_err(|err| format!("{}: {err}", full.display()))?;
    let lua = Lua::new();
    let module = ScriptRuntime::eval_script(&lua, &full, &src, None)
        .map_err(|err| err.to_string())?;
    properties_from(&module).map_err(|err| err.to_string())
}

/// Stops a running instance, calling `on_stop` when possible.
fn stop_instance(lua: &Lua, instance: &mut Instance, log: &mut ScriptLog) {
    if instance.failed || !instance.started {
        return;
    }
    instance.started = false;
    let Ok(module) = lua.registry_value::<Table>(&instance.module) else {
        return;
    };
    let Ok(stop) = module.get::<mlua::Function>("on_stop") else {
        return;
    };
    let Ok(self_table) = lua.registry_value::<Table>(&instance.self_table) else {
        return;
    };
    if let Err(err) = stop.call::<()>(self_table) {
        log.push(format!("script {}: on_stop: {err}", instance.path));
    }
    if let Ok(mut api) = instance.api.lock() {
        for line in api.log.drain(..) {
            log.push(line);
        }
    }
}

fn build_instance(
    lua: &Lua,
    entity: Entity,
    scripted: &ScriptedObject,
    full: &Path,
    src: &str,
    name: &str,
) -> Result<Instance, String> {
    let path = scripted.script.path.as_str();
    let preview =
        ScriptRuntime::eval_script(lua, full, src, None).map_err(|err| err.to_string())?;
    let mut properties = properties_from(&preview).map_err(|err| err.to_string())?;
    for (key, value) in &scripted.script.properties {
        properties.insert(key.clone(), value.clone());
    }
    let (api, self_table) =
        ScriptRuntime::build_api(lua, entity, name, properties).map_err(|err| err.to_string())?;
    let fabr = self_table
        .get::<Table>("fabr")
        .map_err(|err| err.to_string())?;
    let module = ScriptRuntime::eval_script(lua, full, src, Some(fabr))
        .map_err(|err| err.to_string())?;
    let module_key = lua
        .create_registry_value(module)
        .map_err(|err| err.to_string())?;
    let self_key = lua
        .create_registry_value(self_table)
        .map_err(|err| err.to_string())?;
    Ok(Instance {
        object_id: scripted.object_id,
        path: path.to_owned(),
        module: module_key,
        self_table: self_key,
        api,
        started: false,
        failed: false,
    })
}

/// Starts and stops instances to follow the active flag and attached components.
pub fn sync_scripts(
    active: Res<ScriptsActive>,
    root: Res<ScriptRoot>,
    mut runtime: ResMut<ScriptRuntime>,
    mut reload: ResMut<ScriptReload>,
    mut log: ResMut<ScriptLog>,
    mut csharp: ResMut<CSharpScripts>,
    scripted: Query<(Entity, &ScriptedObject)>,
    names: Query<(Entity, &Name)>,
) {
    let ScriptRuntime {
        lua,
        instances,
        scene_requests: _,
        ui: _,
        ui_clicks: _,
    } = &mut *runtime;
    if !active.0 {
        for (_, mut instance) in instances.drain() {
            stop_instance(lua, &mut instance, &mut log);
        }
        csharp::stop_all(&mut csharp, &mut log);
        return;
    }

    let mut live = HashMap::new();
    let mut csharp_live = HashMap::new();
    for (entity, scripted) in &scripted {
        match scripted.script.language {
            ScriptLanguage::Lua => {
                live.insert(entity, scripted.script.path.clone());
            }
            ScriptLanguage::CSharp => {
                csharp_live.insert(
                    entity,
                    (
                        scripted.script.path.clone(),
                        scripted.script.class.clone().unwrap_or_default(),
                    ),
                );
            }
        }
    }
    let stale: Vec<Entity> = instances
        .keys()
        .filter(|entity| !live.contains_key(entity))
        .copied()
        .collect();
    for entity in stale {
        if let Some(mut instance) = instances.remove(&entity) {
            stop_instance(lua, &mut instance, &mut log);
        }
    }
    let csharp_stale: Vec<Entity> = csharp
        .entities()
        .into_iter()
        .filter(|entity| !csharp_live.contains_key(entity))
        .collect();
    for entity in csharp_stale {
        csharp.remove(entity, &mut log);
    }

    let reloads: Vec<String> = std::mem::take(&mut reload.paths);

    let mut entity_names = HashMap::new();
    for (entity, name) in &names {
        entity_names.insert(entity, name.as_str().to_owned());
    }

    for (entity, scripted) in &scripted {
        if scripted.script.language != ScriptLanguage::Lua {
            continue;
        }
        let path = &scripted.script.path;
        let existing = instances.get(&entity);
        let same_path = existing.is_some_and(|instance| &instance.path == path);
        let needs_load = existing
            .is_none_or(|instance| &instance.path != path || reloads.contains(path));
        if !needs_load {
            continue;
        }

        let full = root.0.join(path);
        let src = match std::fs::read_to_string(&full) {
            Ok(src) => src,
            Err(err) => {
                log.push(format!("script {}: {}: {err}", path, full.display()));
                if same_path && reloads.contains(path) {
                    log.push(format!("script {path}: reload failed; keeping the previous version"));
                } else if let Some(mut old) = instances.remove(&entity) {
                    stop_instance(lua, &mut old, &mut log);
                }
                continue;
            }
        };

        let name = entity_names.get(&entity).map_or("", String::as_str);
        match build_instance(lua, entity, scripted, &full, &src, name) {
            Ok(instance) => {
                if let Some(mut old) = instances.remove(&entity) {
                    stop_instance(lua, &mut old, &mut log);
                    if same_path {
                        log.push(format!("script {path}: reloaded"));
                    }
                }
                instances.insert(entity, instance);
            }
            Err(err) => {
                log.push(format!("script {path}: {err}"));
                if same_path && reloads.contains(path) {
                    log.push(format!("script {path}: reload failed; keeping the previous version"));
                } else if let Some(mut old) = instances.remove(&entity) {
                    stop_instance(lua, &mut old, &mut log);
                }
            }
        }
    }

    for (entity, scripted) in &scripted {
        if scripted.script.language != ScriptLanguage::CSharp {
            continue;
        }
        let path = scripted.script.path.as_str();
        let class = scripted.script.class.as_deref().unwrap_or("").trim();
        if class.is_empty() {
            if !csharp.has_failed(path, "") {
                csharp.mark_failed(path, "");
                log.push(format!("script {path}: C# scripts need a class name"));
            }
            continue;
        }
        let reloading = reloads.iter().any(|queued| queued == path);
        let loaded = csharp.matches(entity, path, class);
        let needs_load = !loaded || reloading;
        if !needs_load {
            continue;
        }
        if !reloading && csharp.has_failed(path, class) {
            continue;
        }
        let full = root.0.join(path);
        // the bootstrap keys instances by the entity handle, so the old
        // instance must be stopped and unloaded before the new one loads
        csharp.remove(entity, &mut log);
        match csharp::load(&mut csharp, entity, path, class, &full) {
            Ok(mut instance) => {
                csharp.clear_failed(path, class);
                if let Err(err) = csharp::apply_properties(&mut instance, &scripted.script.properties)
                {
                    log.push(format!("script {path}: {err}"));
                }
                if loaded {
                    log.push(format!("script {path}: reloaded"));
                }
                csharp.insert(entity, instance);
            }
            Err(err) => {
                csharp.mark_failed(path, class);
                log.push(format!("script {path}: {err}"));
            }
        }
    }
}

/// Reads the frame's inputs, leaving the world untouched.
fn build_frame(snapshot: &FrameInput) -> FrameState {
    let FrameInput {
        time,
        keys,
        mouse,
        actions,
        grid,
    } = snapshot;
    let mut pressed = HashMap::new();
    let mut just_pressed = HashMap::new();
    for (name, bindings) in &actions.map {
        pressed.insert(name.clone(), bindings.iter().any(|key| keys.pressed(*key)));
        just_pressed.insert(
            name.clone(),
            bindings.iter().any(|key| keys.just_pressed(*key)),
        );
    }
    let mouse = [
        (MouseButton::Left, 1u8),
        (MouseButton::Right, 2),
        (MouseButton::Middle, 3),
    ]
    .into_iter()
    .filter(|(button, _)| mouse.pressed(*button))
    .map(|(_, id)| id)
    .collect();
    FrameState {
        keys: keys
            .get_pressed()
            .map(|key| format!("{key:?}").to_ascii_lowercase())
            .map(|key| key.strip_prefix("key").map_or(key.clone(), str::to_owned))
            .collect(),
        keys_down: keys
            .get_just_pressed()
            .map(|key| format!("{key:?}").to_ascii_lowercase())
            .map(|key| key.strip_prefix("key").map_or(key.clone(), str::to_owned))
            .collect(),
        actions: pressed,
        actions_down: just_pressed,
        mouse,
        nav: Some(Arc::new(grid.as_deref().cloned().unwrap_or_default())),
        delta: time.delta_secs(),
        elapsed: time.elapsed_secs(),
        ..Default::default()
    }
}

/// Frame inputs read before the scripting pass starts writing to the world.
#[derive(SystemParam)]
pub struct FrameInput<'w> {
    time: Res<'w, Time>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    actions: Res<'w, InputActions>,
    grid: Option<Res<'w, NavGrid>>,
}

/// World-side handles the scripting passes write through.
///
/// Lua and C# queue operations while they run; this bundle owns the queries and
/// resources those operations land in, so both passes share one applier instead
/// of each carrying its own copy of the queries.
#[derive(SystemParam)]
pub struct ApplyTargets<'w, 's> {
    root: Res<'w, ScriptRoot>,
    audio: ResMut<'w, ScriptAudio>,
    transforms: Query<'w, 's, (Entity, &'static mut Transform, Option<&'static Name>)>,
    bodies: Query<'w, 's, (Entity, &'static mut PhysicsBody)>,
    animations: Query<'w, 's, (Entity, &'static mut AnimationSet)>,
    agents: Query<'w, 's, (Entity, &'static mut NavAgent)>,
    controllers: Query<'w, 's, &'static mut KinematicController>,
    commands: Commands<'w, 's>,
}

impl ApplyTargets<'_, '_> {
    /// Reads the world into the frame every script instance sees.
    fn snapshot(&mut self, input: &FrameInput) -> FrameState {
        let mut frame = build_frame(input);
        for (entity, transform, name) in &mut self.transforms {
            frame
                .transforms
                .insert(entity, (transform.translation, transform.rotation));
            if let Some(name) = name {
                frame.names.insert(name.as_str().to_owned(), entity);
            }
        }
        for (entity, body) in &self.bodies {
            frame.velocities.insert(entity, body.velocity);
            frame.angular.insert(entity, body.angular_velocity);
            if body.grounded {
                frame.grounded.insert(entity);
            }
        }
        for (entity, set) in &self.animations {
            frame.anim_clips.insert(entity, set.clips.clone());
        }
        frame
    }

    /// Applies one instance's queued operations to the world.
    fn apply(
        &mut self,
        api: &mut ApiState,
        log: &mut ScriptLog,
        scene_requests: &mut Vec<SceneOp>,
        ui: &mut Vec<UiElement>,
    ) {
        for line in api.log.drain(..) {
            log.push(line);
        }
        for (target, velocity) in api.velocity_ops.drain(..) {
            if let Ok((_, mut body)) = self.bodies.get_mut(target) {
                body.velocity = velocity;
                if velocity.y > 0.0 {
                    body.grounded = false;
                }
            }
        }
        for (target, angular) in api.angular_ops.drain(..) {
            if let Ok((_, mut body)) = self.bodies.get_mut(target) {
                body.angular_velocity = angular;
            }
        }
        for (target, impulse) in api.spin_ops.drain(..) {
            if let Ok((_, mut body)) = self.bodies.get_mut(target) {
                body.apply_angular_impulse(impulse, 1.0);
            }
        }
        for (a, b, rest, stiffness, damping) in api.joint_ops.drain(..) {
            self.commands.entity(a).insert(DistanceJoint {
                a,
                b,
                rest: rest.max(0.05),
                stiffness: stiffness.clamp(0.0, 200.0),
                damping: damping.clamp(0.0, 50.0),
            });
        }
        for (entity, op) in api.animation_ops.drain(..) {
            if let Ok((_, mut set)) = self.animations.get_mut(entity) {
                match op {
                    AnimationOp::Play(name) => {
                        set.play(&name);
                    }
                    AnimationOp::Stop => set.stop(),
                }
            }
        }
        for (entity, op) in api.nav_ops.drain(..) {
            match op {
                NavOp::Go { goal, speed } => {
                    if let Ok((_, mut agent)) = self.agents.get_mut(entity) {
                        agent.speed = speed;
                        agent.go_to(goal);
                    } else {
                        let mut agent = NavAgent::default();
                        agent.speed = speed;
                        agent.go_to(goal);
                        self.commands.entity(entity).insert(agent);
                    }
                }
                NavOp::Stop => {
                    if let Ok((_, mut agent)) = self.agents.get_mut(entity) {
                        agent.stop();
                    }
                }
            }
        }
        for (entity, op) in api.controller_ops.drain(..) {
            if let Ok(mut controller) = self.controllers.get_mut(entity) {
                match op {
                    ControllerOp::Walk(x, z) => controller.walk(x, z),
                    ControllerOp::Jump => controller.jump(),
                }
            }
        }
        for element in api.ui.drain(..) {
            ui.push(element);
        }
        for op in api.scene_ops.drain(..) {
            match &op {
                SceneOp::Load(path) => log.push(format!("scene: load requested {path}")),
                SceneOp::Additive { id, path } => {
                    log.push(format!("scene: additive load requested {path} as layer {id}"));
                }
                SceneOp::Unload(id) => log.push(format!("scene: unload requested for layer {id}")),
                SceneOp::Reload => log.push("scene: reload requested".to_string()),
            }
            scene_requests.push(op);
        }
        for request in api.audio.drain(..) {
            if self.root.0.join(&request.path).is_file() {
                log.push(format!(
                    "script audio: play {} @ {:.2} ({})",
                    request.path,
                    request.volume,
                    request.bus.label()
                ));
                self.audio.requests.push(request);
            } else {
                log.push(format!("script audio: missing asset {}", request.path));
            }
        }
        write_ops(&api.ops, &mut self.transforms);
    }
}

/// Runs `on_start` once and `on_update` for every live instance.
pub fn update_scripts(
    active: Res<ScriptsActive>,
    input: FrameInput,
    mut runtime: ResMut<ScriptRuntime>,
    mut log: ResMut<ScriptLog>,
    mut frame_out: ResMut<ScriptFrame>,
    mut targets: ApplyTargets,
) {
    if !active.0 {
        return;
    }
    let ScriptRuntime {
        lua,
        instances,
        scene_requests,
        ui,
        ui_clicks,
    } = &mut *runtime;
    let mut order: Vec<(u64, Entity)> = instances
        .iter()
        .map(|(entity, instance)| (instance.object_id, *entity))
        .collect();
    order.sort_unstable();

    let mut frame = targets.snapshot(&input);
    frame.ui_clicks = std::mem::take(ui_clicks);
    let frame = Arc::new(frame);
    frame_out.0 = Arc::clone(&frame);
    if order.is_empty() {
        return;
    }

    for (_, entity) in order {
        let Some(instance) = instances.get_mut(&entity) else {
            continue;
        };
        if instance.failed {
            continue;
        }
        let Ok(module) = lua.registry_value::<Table>(&instance.module) else {
            continue;
        };
        let Ok(self_table) = lua.registry_value::<Table>(&instance.self_table) else {
            continue;
        };
        instance
            .api
            .lock()
            .unwrap()
            .reset_frame(Arc::clone(&frame));

        let mut result = Ok(());
        if !instance.started {
            if let Ok(start) = module.get::<mlua::Function>("on_start") {
                result = start.call::<()>(self_table.clone());
            }
            instance.started = true;
        }
        if result.is_ok()
            && let Ok(update) = module.get::<mlua::Function>("on_update")
        {
            result = update.call::<()>((self_table.clone(), frame.delta));
        }
        if let Err(err) = result {
            log.push(format!("script {}: {err}", instance.path));
            instance.failed = true;
            continue;
        }

        targets.apply(&mut instance.api.lock().unwrap(), &mut log, scene_requests, ui);
    }
}

/// Writes queued transforms back to the world.
fn write_ops(
    ops: &[(Entity, Vec3, Quat)],
    transforms: &mut Query<(Entity, &mut Transform, Option<&Name>)>,
) {
    for (target, translation, rotation) in ops {
        if let Ok((_, mut transform, _)) = transforms.get_mut(*target) {
            transform.translation = *translation;
            transform.rotation = *rotation;
        }
    }
}

/// Runs C# scripts after the Lua pass against the same frame's transforms.
pub fn update_csharp_scripts(
    active: Res<ScriptsActive>,
    frame: Res<ScriptFrame>,
    mut runtime: ResMut<ScriptRuntime>,
    mut scripts: ResMut<CSharpScripts>,
    mut log: ResMut<ScriptLog>,
    mut targets: ApplyTargets,
) {
    if !active.0 || scripts.is_empty() {
        return;
    }
    let runtime = &mut *runtime;
    let (scene_requests, ui) = (&mut runtime.scene_requests, &mut runtime.ui);
    let mut entities = scripts.entities();
    entities.sort_unstable();
    for entity in entities {
        let Some(instance) = scripts.instance_mut(entity) else {
            continue;
        };
        csharp::step(instance, &frame.0, frame.0.delta);
        targets.apply(instance.api_mut(), &mut log, scene_requests, ui);
    }
}

/// Spawns an [`AudioPlayer`] that despawns when the clip ends.
fn spawn_audio(commands: &mut Commands, source: Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer::new(source),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}

/// Turns queued script requests into [`AudioPlayer`] entities.
pub fn play_script_audio(
    mut commands: Commands,
    mut audio: ResMut<ScriptAudio>,
    mix: Res<AudioMix>,
    mut log: ResMut<ScriptLog>,
    assets: Res<AssetServer>,
) {
    for request in audio.requests.drain(..) {
        let volume = effective_volume(&request, &mix);
        log.push(format!(
            "script audio: mixed {} at {:.2} ({})",
            request.path,
            volume,
            request.bus.label()
        ));
        spawn_audio(&mut commands, assets.load(request.path), volume);
    }
}

/// Moves scene requests raised by scripts into [`SceneRequests`].
pub fn publish_scene_requests(
    mut runtime: ResMut<ScriptRuntime>,
    mut requests: ResMut<SceneRequests>,
) {
    for op in runtime.scene_requests.drain(..) {
        match op {
            SceneOp::Load(path) => {
                requests.load = Some(path);
                requests.reload = false;
            }
            SceneOp::Additive { id, path } => requests.additive.push((id, path)),
            SceneOp::Unload(id) => requests.unload.push(id),
            SceneOp::Reload => {
                requests.reload = true;
                requests.load = None;
            }
        }
    }
}

/// Publishes HUD elements and stores app-side button clicks.
pub fn publish_script_ui(mut runtime: ResMut<ScriptRuntime>, mut ui: ResMut<ScriptUi>) {
    let next = std::mem::take(&mut runtime.ui);
    if ui.elements != next {
        ui.elements = next;
    }
    if !ui.clicks.is_empty() {
        runtime.ui_clicks = std::mem::take(&mut ui.clicks);
    }
}

/// Plugin wiring the script resources and systems into an app.
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptsActive>()
            .init_resource::<ScriptRoot>()
            .init_resource::<ScriptLog>()
            .init_resource::<ScriptReload>()
            .init_resource::<InputActions>()
            .init_resource::<ScriptAudio>()
            .init_resource::<SceneRequests>()
            .init_resource::<ScriptUi>()
            .init_resource::<AudioMix>()
            .init_resource::<ScriptRuntime>()
            .init_resource::<CSharpScripts>()
            .init_resource::<ScriptFrame>()
            .add_systems(Update, sync_scripts)
            .add_systems(
                Update,
                update_scripts.after(eme_game_runtime::run_behaviours),
            )
            .add_systems(
                Update,
                update_csharp_scripts
                    .after(update_scripts)
                    .after(eme_game_runtime::run_behaviours),
            )
            .add_systems(
                Update,
                play_script_audio.after(update_csharp_scripts),
            )
            .add_systems(
                Update,
                publish_scene_requests.after(update_csharp_scripts),
            )
            .add_systems(
                Update,
                publish_script_ui.after(update_csharp_scripts),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_ecs::system::RunSystemOnce;

    #[test]
    fn api_state_reads_back_its_own_writes() {
        let mut api = ApiState::default();
        let entity = api.self_entity;
        api.set_translation(entity, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(api.translation(entity), Vec3::new(1.0, 2.0, 3.0));
        api.translate(entity, Vec3::X);
        assert_eq!(api.translation(entity), Vec3::new(2.0, 2.0, 3.0));
        api.rotate_y(entity, core::f32::consts::FRAC_PI_2);
        assert!((api.rotation(entity).y - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        api.set_angular_velocity(entity, Vec3::Y);
        assert_eq!(api.angular_velocity(entity), Vec3::Y);
        api.set_velocity(entity, Vec3::Z);
        assert_eq!(api.velocity(entity), Vec3::Z);
        assert!(!api.grounded(entity));
        assert_eq!(api.ops.len(), 1);
        assert_eq!(api.velocity_ops.len(), 1);
        assert_eq!(api.angular_ops.len(), 1);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-script-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn world(root: &Path) -> World {
        let mut world = World::new();
        world.insert_resource(ScriptsActive(true));
        world.insert_resource(ScriptRoot(root.to_path_buf()));
        world.insert_resource(ScriptLog::default());
        world.insert_resource(ScriptReload::default());
        world.insert_resource(InputActions::default());
        world.insert_resource(ScriptAudio::default());
        world.insert_resource(SceneRequests::default());
        world.insert_resource(ScriptUi::default());
        world.insert_resource(NavGrid::default());
        world.insert_resource(AudioMix::default());
        world.insert_resource(ScriptRuntime::default());
        world.insert_resource(CSharpScripts::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(ScriptFrame::default());
        world
    }

    fn scripted(world: &mut World, path: &str, properties: BTreeMap<String, ScriptValue>) -> Entity {
        let entity = world
            .spawn((Name::new("Spinner"), Transform::default()))
            .id();
        world.entity_mut(entity).insert(ScriptedObject {
            object_id: 1,
            script: ScriptRef {
                path: path.to_owned(),
                properties,
                ..Default::default()
            },
        });
        entity
    }

    #[test]
    fn a_script_updates_its_transform() {
        let dir = scratch("move");
        std::fs::write(
            dir.join("spin.lua"),
            "return { on_update = function(self, dt) self:translate(2.0 * dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "spin.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        {
            let mut time = world.resource_mut::<Time>();
            time.advance_by(core::time::Duration::from_secs_f32(0.5));
        }
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 1.0);
    }

    #[test]
    fn properties_override_defaults() {
        let dir = scratch("props");
        std::fs::write(
            dir.join("spin.lua"),
            "return { properties = { speed = 1.0 }, on_update = function(self, dt) self:translate(self.properties.speed * dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let mut props = BTreeMap::new();
        props.insert("speed".to_string(), ScriptValue::Number(4.0));
        let entity = scripted(&mut world, "spin.lua", props);
        world.run_system_once(sync_scripts).unwrap();
        world.resource_mut::<Time>().advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 2.0);
    }

    #[test]
    fn a_script_error_is_logged_and_does_not_panic() {
        let dir = scratch("error");
        std::fs::write(dir.join("bad.lua"), "return { on_update = function() error('boom') end }").unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "bad.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        assert!(world.resource::<ScriptLog>().lines().iter().any(|line| line.contains("boom")));
        assert!(world.get::<Transform>(entity).is_some());
    }

    #[test]
    fn lifecycle_callbacks_run_in_order_and_stop_on_deactivate() {
        let dir = scratch("life");
        std::fs::write(
            dir.join("life.lua"),
            "return { on_start = function(self) fabr.log('start') end, on_update = function(self) fabr.log('update') end, on_stop = function(self) fabr.log('stop') end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "life.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        world.resource_mut::<ScriptsActive>().0 = false;
        world.run_system_once(sync_scripts).unwrap();
        let lines = world.resource::<ScriptLog>().lines();
        assert_eq!(lines, ["start", "update", "stop"]);
    }

    #[test]
    fn hot_reload_reloads_only_that_script() {
        let dir = scratch("reload");
        std::fs::write(
            dir.join("spin.lua"),
            "return { on_update = function(self, dt) self:translate(1.0 * dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "spin.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.resource_mut::<Time>().advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 0.5);

        std::fs::write(
            dir.join("spin.lua"),
            "return { on_update = function(self, dt) self:translate(3.0 * dt, 0, 0) end }",
        )
        .unwrap();
        world.resource_mut::<ScriptReload>().request("spin.lua");
        world.run_system_once(sync_scripts).unwrap();
        world.resource_mut::<Time>().advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 2.0);
    }

    #[test]
    fn a_broken_reload_keeps_the_previous_version_running() {
        let dir = scratch("broken-reload");
        std::fs::write(
            dir.join("spin.lua"),
            "return { on_update = function(self, dt) self:translate(1.0 * dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "spin.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 0.5);

        std::fs::write(dir.join("spin.lua"), "return { on_update = function(self, dt) ").unwrap();
        world.resource_mut::<ScriptReload>().request("spin.lua");
        world.run_system_once(sync_scripts).unwrap();
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("keeping the previous version"), "{log}");

        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 1.0);
    }

    #[test]
    fn hot_reload_preserves_property_overrides() {
        let dir = scratch("reload-props");
        std::fs::write(
            dir.join("move.lua"),
            "return { properties = { speed = 1.0 }, on_update = function(self, dt) self:translate(self.properties.speed * dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let mut props = BTreeMap::new();
        props.insert("speed".to_string(), ScriptValue::Number(2.0));
        let entity = scripted(&mut world, "move.lua", props);
        world.run_system_once(sync_scripts).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 1.0);

        std::fs::write(
            dir.join("move.lua"),
            "return { properties = { speed = 9.0 }, on_update = function(self, dt) self:translate(self.properties.speed * dt, 0, 0) end }",
        )
        .unwrap();
        world.resource_mut::<ScriptReload>().request("move.lua");
        world.run_system_once(sync_scripts).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 2.0);
    }

    #[test]
    fn a_script_queues_audio_and_reports_missing_assets() {
        let dir = scratch("audio");
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join("audio/beep.wav"), b"RIFF").unwrap();
        std::fs::write(
            dir.join("sound.lua"),
            r#"
return {
    properties = { volume = 0.5 },
    on_start = function(self)
        fabr.audio.play("audio/beep.wav", self.properties.volume)
        fabr.audio.play("audio/missing.wav")
    end,
}
"#,
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "sound.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();

        let audio = world.resource::<ScriptAudio>();
        assert_eq!(audio.requests.len(), 1);
        assert_eq!(audio.requests[0].path, "audio/beep.wav");
        assert!((audio.requests[0].volume - 0.5).abs() < 1e-6);
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("play audio/beep.wav @ 0.50 (sfx)"), "{log}");
        assert!(log.contains("missing asset audio/missing.wav"), "{log}");
    }

    #[test]
    fn audio_bus_gains_scale_requests() {
        let mix = AudioMix {
            sfx: 0.5,
            music: 0.25,
        };
        let music = AudioRequest {
            path: "audio/song.ogg".into(),
            volume: 0.8,
            bus: AudioBus::Music,
        };
        assert!((effective_volume(&music, &mix) - 0.2).abs() < 1e-6);
        let sfx = AudioRequest {
            bus: AudioBus::Sfx,
            ..music.clone()
        };
        assert!((effective_volume(&sfx, &mix) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_music_bus_reaches_the_queue() {
        let dir = scratch("audio-bus");
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join("audio/beep.wav"), b"RIFF").unwrap();
        std::fs::write(
            dir.join("music.lua"),
            "return { on_start = function() fabr.audio.play('audio/beep.wav', 1.0, 'music') end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "music.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let audio = world.resource::<ScriptAudio>();
        assert_eq!(audio.requests.len(), 1);
        assert_eq!(audio.requests[0].bus, AudioBus::Music);
    }

    #[test]
    fn audio_defaults_to_full_volume_and_clamps() {
        let dir = scratch("audio-volume");
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join("audio/beep.wav"), b"RIFF").unwrap();
        std::fs::write(
            dir.join("loud.lua"),
            "return { on_start = function() fabr.audio.play('audio/beep.wav'); fabr.audio.play('audio/beep.wav', 99) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "loud.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let audio = world.resource::<ScriptAudio>();
        assert_eq!(audio.requests.len(), 2);
        assert!((audio.requests[0].volume - 1.0).abs() < 1e-6);
        assert!((audio.requests[1].volume - 4.0).abs() < 1e-6);
    }

    #[test]
    fn audio_requests_spawn_a_despawning_player_at_the_requested_volume() {
        #[derive(Resource)]
        struct Cue {
            handle: Handle<AudioSource>,
            volume: f32,
        }

        fn spawn(mut commands: Commands, cue: Res<Cue>) {
            spawn_audio(&mut commands, cue.handle.clone(), cue.volume);
        }

        fn check(players: Query<(&AudioPlayer<AudioSource>, &PlaybackSettings)>) {
            let (player, settings) = players.single().unwrap();
            assert!(matches!(player.0, Handle::Uuid(..)));
            assert!(matches!(
                settings.mode,
                eme_audio::PlaybackMode::Despawn
            ));
            match settings.volume {
                Volume::Linear(volume) => assert!((volume - 0.25).abs() < 1e-6),
                _ => panic!("expected linear volume"),
            }
        }

        let mut app = App::new();
        app.insert_resource(Cue {
            handle: Handle::default(),
            volume: 0.25,
        })
        .add_systems(Update, (spawn, check).chain());
        app.update();
    }

    #[test]
    fn a_script_reads_physics_state() {
        let dir = scratch("physics-read");
        std::fs::write(
            dir.join("probe.lua"),
            r#"
return {
    on_start = function(self)
        local _, vy, _ = self:get_velocity()
        fabr.log("vy=" .. tostring(vy) .. " grounded=" .. tostring(self:is_grounded()))
    end,
}
"#,
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "probe.lua", BTreeMap::new());
        world.entity_mut(entity).insert(PhysicsBody {
            dynamic: true,
            half_extents: Vec3::splat(0.5),
            velocity: Vec3::new(0.0, -3.0, 0.0),
            angular_velocity: Vec3::ZERO,
            grounded: true,
        });
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("vy=-3"), "{log}");
        assert!(log.contains("grounded=true"), "{log}");
    }

    #[test]
    fn a_script_sets_body_velocity() {
        let dir = scratch("physics-write");
        std::fs::write(
            dir.join("launch.lua"),
            "return { on_start = function(self) self:set_velocity(0, 5, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "launch.lua", BTreeMap::new());
        world.entity_mut(entity).insert(PhysicsBody {
            dynamic: true,
            half_extents: Vec3::splat(0.5),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            grounded: true,
        });
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let body = world
            .get::<PhysicsBody>(entity)
            .expect("body survives the script");
        assert_eq!(body.velocity, Vec3::new(0.0, 5.0, 0.0));
        assert!(!body.grounded);
    }

    fn animated(entity: Entity, world: &mut World) {
        world.entity_mut(entity).insert(AnimationSet {
            clips: vec!["Spin".into(), "Walk".into()],
            ..Default::default()
        });
    }

    #[test]
    fn a_script_lists_and_plays_animation_clips() {
        let dir = scratch("animation");
        std::fs::write(
            dir.join("anim.lua"),
            "return { on_start = function(self) fabr.log('clips=' .. #self:animation_clips() .. ' ok=' .. tostring(self:play_animation('Spin'))) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "anim.lua", BTreeMap::new());
        animated(entity, &mut world);
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("clips=2 ok=true"), "{log}");
        let set = world.get::<AnimationSet>(entity).unwrap();
        assert_eq!(set.selected.as_deref(), Some("Spin"));
        assert!(set.playing);
    }

    #[test]
    fn a_script_reports_missing_animation_clips() {
        let dir = scratch("animation-missing");
        std::fs::write(
            dir.join("anim.lua"),
            "return { on_start = function(self) fabr.log('ok=' .. tostring(self:play_animation('Fly'))) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "anim.lua", BTreeMap::new());
        animated(entity, &mut world);
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("ok=false"), "{log}");
        assert!(log.contains("missing clip Fly"), "{log}");
        let set = world.get::<AnimationSet>(entity).unwrap();
        assert!(!set.playing);
    }

    #[test]
    fn a_script_stops_animation() {
        let dir = scratch("animation-stop");
        std::fs::write(
            dir.join("anim.lua"),
            "return { on_start = function(self) self:stop_animation() end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "anim.lua", BTreeMap::new());
        world.entity_mut(entity).insert(AnimationSet {
            clips: vec!["Spin".into()],
            selected: Some("Spin".into()),
            playing: true,
            ..Default::default()
        });
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let set = world.get::<AnimationSet>(entity).unwrap();
        assert!(!set.playing);
    }

    #[test]
    fn a_script_finds_a_path_on_the_nav_grid() {
        let dir = scratch("nav-path");
        std::fs::write(
            dir.join("nav.lua"),
            "return { on_start = function(self) local p = fabr.nav.find_path(-5, 0, 0, 5, 0, 0) fabr.log('points=' .. #p .. ' first=' .. tostring(p[1].x)) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "nav.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("points="), "{log}");
        assert!(!log.contains("points=0"), "expected a path: {log}");
    }

    #[test]
    fn a_script_moves_an_agent_and_stops_it() {
        let dir = scratch("nav-agent");
        std::fs::write(
            dir.join("walk.lua"),
            "return { on_start = function(self) self:move_to(4, 0, 0, 6) end, on_update = function(self, dt) if self.t then self:stop_moving() end self.t = (self.t or 0) + 1 end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "walk.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let agent = world
            .get::<NavAgent>(entity)
            .expect("move_to inserts a NavAgent");
        assert_eq!(agent.goal, Some(Vec3::new(4.0, 0.0, 0.0)));
        assert!((agent.speed - 6.0).abs() < 1e-6);

        world.run_system_once(update_scripts).unwrap();
        let agent = world.get::<NavAgent>(entity).unwrap();
        assert_eq!(agent.goal, None, "stop_moving clears the goal");
    }

    #[test]
    fn a_script_requests_scene_load_and_reload() {
        let dir = scratch("scene-requests");
        std::fs::write(
            dir.join("go.lua"),
            "return { on_start = function(self) self.loaded = fabr.scene.load('scenes/level2.ember') end, on_update = function(self) if self.t then fabr.scene.reload() end self.t = (self.t or 0) + 1 end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "go.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(publish_scene_requests).unwrap();
        let requests = world.resource::<SceneRequests>();
        assert_eq!(requests.load.as_deref(), Some("scenes/level2.ember"));
        assert!(!requests.reload);

        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(publish_scene_requests).unwrap();
        let requests = world.resource::<SceneRequests>();
        assert!(requests.reload);
        assert_eq!(requests.load, None);
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("scene: load requested"), "{log}");
        assert!(log.contains("scene: reload requested"), "{log}");
    }

    #[test]
    fn a_script_draws_and_clears_hud_elements() {
        let dir = scratch("ui");
        std::fs::write(dir.join("hud.lua"), "return { on_start = function() fabr.ui.panel(0.1,0.1,0.3,0.2,1,0,0,1) fabr.ui.text(0.1,0.4,20,'score') fabr.ui.button(7,0.1,0.6,0.2,0.1,'Restart') end }").unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "hud.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(publish_script_ui).unwrap();
        let ui = world.resource::<ScriptUi>();
        assert_eq!(ui.elements.len(), 3);
        assert_eq!(ui.button_at(0.2, 0.65), Some(7));
        assert_eq!(ui.button_at(0.9, 0.9), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clicks_reach_the_script_once() {
        let dir = scratch("ui-click");
        std::fs::write(dir.join("hud.lua"), "return { on_start = function() fabr.ui.button(3,0,0,0.5,0.5,'go') end, on_update = function(self) if fabr.ui.clicked(3) then fabr.log('clicked 3') end end }").unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "hud.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.resource_mut::<ScriptUi>().clicks.push(3);
        world.run_system_once(publish_script_ui).unwrap();
        world.run_system_once(update_scripts).unwrap();
        assert!(world.resource::<ScriptLog>().lines().join("\n").contains("clicked 3"));
        assert!(world.resource::<ScriptUi>().clicks.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_script_walks_and_jumps_a_controller() {
        let dir = scratch("controller");
        std::fs::write(
            dir.join("walk.lua"),
            "return { on_start = function(self) self:walk(1, 0) self:jump() end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "walk.lua", BTreeMap::new());
        world
            .entity_mut(entity)
            .insert(KinematicController::new(Vec3::splat(0.5)));
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let controller = world.get::<KinematicController>(entity).unwrap();
        assert!((controller.desired.x - 1.0).abs() < 1e-6);
        assert!(controller.jump);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_script_adds_and_unloads_a_scene_layer() {
        let dir = scratch("scene-layers");
        std::fs::write(
            dir.join("layers.lua"),
            "return { on_start = function(self) local id = fabr.scene.load_additive('scenes/extra.ember') fabr.scene.unload(id) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        scripted(&mut world, "layers.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(publish_scene_requests).unwrap();
        let requests = world.resource::<SceneRequests>();
        assert_eq!(requests.additive.len(), 1);
        let (id, path) = &requests.additive[0];
        assert!(path.ends_with("scenes/extra.ember"));
        assert!(*id >= 1);
        assert_eq!(requests.unload, vec![*id]);
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("additive load requested"), "{log}");
        assert!(log.contains("unload requested"), "{log}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_script_sets_and_reads_angular_velocity() {
        let dir = scratch("spin-api");
        std::fs::write(
            dir.join("spin.lua"),
            "return { on_start = function(self) self:set_angular_velocity(0, 2, 0) self:add_spin(0, 3, 0) end, on_update = function(self) local x, y, z = self:get_angular_velocity() fabr.log(string.format('spin %.2f %.2f %.2f', x, y, z)) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let entity = scripted(&mut world, "spin.lua", BTreeMap::new());
        world.entity_mut(entity).insert(PhysicsBody {
            dynamic: true,
            half_extents: Vec3::splat(0.5),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            grounded: false,
        });
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let body = world.get::<PhysicsBody>(entity).unwrap();
        // 2 rad/s from the setter plus 3 / (1/6) rad/s from the impulse
        assert!((body.angular_velocity.y - 20.0).abs() < 1e-3, "{:?}", body.angular_velocity);
    }

    #[test]
    fn a_script_creates_a_distance_joint_between_names() {
        let dir = scratch("joint-api");
        std::fs::write(
            dir.join("joint.lua"),
            "return { on_update = function(self) fabr.log(tostring(fabr.joint.create('Left', 'Right', 2, 8, 1))) fabr.log(tostring(fabr.joint.create('Left', 'Missing', 2, 8, 1))) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let left = world.spawn((Name::new("Left"), Transform::default())).id();
        let right = world.spawn((Name::new("Right"), Transform::default())).id();
        world.entity_mut(left).insert(PhysicsBody {
            dynamic: true,
            half_extents: Vec3::splat(0.5),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            grounded: false,
        });
        scripted(&mut world, "joint.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        let joint = world.get::<DistanceJoint>(left).expect("joint inserted");
        assert_eq!((joint.a, joint.b), (left, right));
        assert!((joint.rest - 2.0).abs() < 1e-6);
        let log = world.resource::<ScriptLog>().lines().join("\n");
        assert!(log.contains("true"), "{log}");
        assert!(log.contains("false"), "{log}");
    }

    #[test]
    fn property_defaults_are_read_without_running_lifecycle() {
        let dir = scratch("defaults");
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("scripts/props.lua"),
            "return { properties = { speed = 2.5, label = 'x', enabled = true }, on_start = function() error('must not run') end }",
        )
        .unwrap();
        let defaults = script_property_defaults(&dir, "scripts/props.lua").unwrap();
        assert_eq!(defaults.get("speed"), Some(&ScriptValue::Number(2.5)));
        assert_eq!(defaults.get("label"), Some(&ScriptValue::Text("x".into())));
        assert_eq!(defaults.get("enabled"), Some(&ScriptValue::Bool(true)));
    }

    #[test]
    fn input_actions_reach_scripts() {
        let dir = scratch("actions");
        std::fs::write(
            dir.join("act.lua"),
            "return { on_update = function(self, dt) if fabr.input.action('jump') then self:translate(1.0, 0, 0) end end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let mut names = BTreeMap::new();
        names.insert("jump".to_string(), vec!["Space".to_string()]);
        world.insert_resource(InputActions::from_names(&names).unwrap());
        world.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        let entity = scripted(&mut world, "act.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 1.0);
    }

    #[test]
    fn unknown_action_keys_are_rejected() {
        let mut names = BTreeMap::new();
        names.insert("jump".to_string(), vec!["Banana".to_string()]);
        assert!(InputActions::from_names(&names).is_err());
    }

    /// The CLR host and the managed bootstrap are process-wide, so the C#
    /// scene tests must not run at the same time.
    fn csharp_guard() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::Mutex;
        static GUARD: Mutex<()> = Mutex::new(());
        GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn dotnet_available() -> bool {
        std::process::Command::new("dotnet")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    }

    fn run_dotnet(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("dotnet")
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "dotnet {args:?} failed");
    }

    /// Builds the managed bootstrap once per test process.
    fn bindings() -> PathBuf {
        static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        BUILT
            .get_or_init(|| {
                let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../sdk/csharp/Fabr.Bindings");
                let out = std::env::temp_dir()
                    .join(format!("fabr-bindings-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&out);
                run_dotnet(
                    &sdk,
                    &["build", "-c", "Release", "-o", out.to_str().unwrap()],
                );
                out
            })
            .clone()
    }

    fn game_script_source(speed: f32) -> String {
        format!(
            r#"
using System.Numerics;
using Fabr.Bindings;

namespace Game;

public sealed class Spin : FabrScript
{{
    private float elapsed;

    public override void OnStart() => Log("csharp start");

    public override void OnUpdate(float dt)
    {{
        Translate(new Vector3({speed} * dt, 0.0f, 0.0f));
        elapsed += dt;
        if (elapsed > 0.25f)
        {{
            elapsed = 0.0f;
            Log($"csharp x={{Position.X:F2}}");
        }}
    }}

    public override void OnStop() => Log("csharp stop");
}}
"#
        )
    }

    fn build_source(dir: &Path, bindings: &Path, source: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("Game.csproj"),
            format!(
                r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Library</OutputType>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  </PropertyGroup>
  <ItemGroup>
    <Reference Include="Fabr.Bindings">
      <HintPath>{}</HintPath>
    </Reference>
  </ItemGroup>
</Project>
"#,
                bindings.join("Fabr.Bindings.dll").display()
            ),
        )
        .unwrap();
        std::fs::write(dir.join("Script.cs"), source).unwrap();
        run_dotnet(dir, &["build", "-c", "Release", "-o", "out"]);
        dir.join("out/Game.dll")
    }

    fn build_game_script(dir: &Path, bindings: &Path, speed: f32) -> PathBuf {
        build_source(dir, bindings, &game_script_source(speed))
    }

    fn stage_managed(root: &Path, game: &Path, bindings: &Path) {
        let managed = root.join("scripts/managed");
        std::fs::create_dir_all(&managed).unwrap();
        for name in ["Fabr.Bindings.dll", "Fabr.Bindings.runtimeconfig.json"] {
            std::fs::copy(bindings.join(name), managed.join(name)).unwrap();
        }
        std::fs::copy(game, managed.join("Game.dll")).unwrap();
    }

    fn csharp_scripted(world: &mut World, path: &str, class: &str) -> Entity {
        csharp_scripted_with(world, path, class, BTreeMap::new())
    }

    fn csharp_scripted_with(
        world: &mut World,
        path: &str,
        class: &str,
        properties: BTreeMap<String, ScriptValue>,
    ) -> Entity {
        let entity = world
            .spawn((Name::new("Managed"), Transform::default()))
            .id();
        world.entity_mut(entity).insert(ScriptedObject {
            object_id: 2,
            script: ScriptRef {
                path: path.to_owned(),
                properties,
                language: ScriptLanguage::CSharp,
                class: Some(class.to_owned()),
            },
        });
        entity
    }

    #[test]
    fn csharp_and_lua_scripts_run_side_by_side() {
        let _guard = csharp_guard();
        if !dotnet_available() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "dotnet unavailable; skipping the C# scene test"
            );
            return;
        }
        let dir = scratch("csharp");
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("scripts/spin.lua"),
            "return { on_update = function(self, dt) self:translate(2.0 * dt, 0, 0) end }",
        )
        .unwrap();
        let bindings = bindings();
        let game = build_game_script(&dir.join("project"), &bindings, 2.0);
        stage_managed(&dir, &game, &bindings);

        let mut world = world(&dir);
        let lua = scripted(&mut world, "scripts/spin.lua", BTreeMap::new());
        let managed = csharp_scripted(&mut world, "scripts/managed/Game.dll", "Game.Spin");
        world.run_system_once(sync_scripts).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(0.5));
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();

        // the same contract call at the same speed moves both languages equally
        assert_eq!(world.get::<Transform>(lua).unwrap().translation.x, 1.0);
        assert_eq!(world.get::<Transform>(managed).unwrap().translation.x, 1.0);
        let lines = world.resource::<ScriptLog>().lines().to_vec();
        assert!(lines.contains(&"csharp start".to_owned()), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("script scripts/managed")), "{lines:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reloading_a_rebuilt_csharp_assembly_uses_the_new_code() {
        let _guard = csharp_guard();
        if !dotnet_available() {
            return;
        }
        let dir = scratch("csharp-reload");
        std::fs::create_dir_all(dir.join("scripts/managed")).unwrap();
        let bindings = bindings();
        let project = dir.join("project");
        let game = build_game_script(&project, &bindings, 1.0);
        stage_managed(&dir, &game, &bindings);

        let mut world = world(&dir);
        let managed = csharp_scripted(&mut world, "scripts/managed/Game.dll", "Game.Spin");
        world.run_system_once(sync_scripts).unwrap();
        advance(&mut world, 0.5);
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();
        let first = world.get::<Transform>(managed).unwrap().translation.x;
        assert!((first - 0.5).abs() < 1e-4, "{first}");

        // rebuild straight into the staged folder, the way the editor and the
        // README build a game assembly
        std::fs::write(project.join("Script.cs"), game_script_source(4.0)).unwrap();
        let staged = dir.join("scripts/managed");
        run_dotnet(
            &project,
            &["build", "-c", "Release", "-o", staged.to_str().unwrap()],
        );
        world
            .resource_mut::<ScriptReload>()
            .request("scripts/managed/Game.dll");
        world.run_system_once(sync_scripts).unwrap();
        advance(&mut world, 0.5);
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();
        let second = world.get::<Transform>(managed).unwrap().translation.x;
        assert!((second - 2.5).abs() < 1e-4, "expected 2.5 after reload, got {second}");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn property_script_source() -> &'static str {
        r#"
using System.Numerics;
using Fabr.Bindings;

namespace Game;

public sealed class Spin : FabrScript
{
    [Property]
    public float Speed { get; set; } = 2.0f;

    [Property]
    public string Label { get; set; } = "spin";

    [Property]
    public bool Enabled { get; set; } = true;

    [Property]
    public int Steps { get; set; } = 3;

    public float Plain { get; set; } = 9.0f;

    public override void OnStart() => Log($"props {Speed:F1}|{Label}|{Enabled}|{Steps}");

    public override void OnUpdate(float dt) =>
        Translate(new Vector3(Speed * dt, 0.0f, 0.0f));
}
"#
    }

    #[test]
    fn inspect_reads_declared_properties_without_starting_the_script() {
        let _guard = csharp_guard();
        if !dotnet_available() {
            return;
        }
        let dir = scratch("csharp-inspect");
        std::fs::create_dir_all(dir.join("scripts/managed")).unwrap();
        let bindings = bindings();
        let game = build_source(&dir.join("project"), &bindings, property_script_source());
        stage_managed(&dir, &game, &bindings);

        let mut scripts = CSharpScripts::default();
        let defaults = scripts
            .inspect(&dir, "scripts/managed/Game.dll", "Game.Spin")
            .unwrap();
        let names: Vec<&str> = defaults.keys().map(String::as_str).collect();
        assert_eq!(names, ["Enabled", "Label", "Speed", "Steps"]);
        assert_eq!(defaults["Speed"], ScriptValue::Number(2.0));
        assert_eq!(defaults["Label"], ScriptValue::Text("spin".into()));
        assert_eq!(defaults["Enabled"], ScriptValue::Bool(true));
        assert_eq!(defaults["Steps"], ScriptValue::Number(3.0));

        let err = scripts
            .inspect(&dir, "scripts/managed/Game.dll", "Game.Nope")
            .unwrap_err();
        assert!(err.contains("Game.Nope"), "{err}");
        let err = scripts
            .inspect(&dir, "scripts/managed/Game.dll", "  ")
            .unwrap_err();
        assert!(err.contains("class name"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn csharp_property_overrides_reach_the_script() {
        let _guard = csharp_guard();
        if !dotnet_available() {
            return;
        }
        let dir = scratch("csharp-props");
        std::fs::create_dir_all(dir.join("scripts/managed")).unwrap();
        let bindings = bindings();
        let game = build_source(&dir.join("project"), &bindings, property_script_source());
        stage_managed(&dir, &game, &bindings);

        let mut props = BTreeMap::new();
        props.insert("Speed".to_string(), ScriptValue::Number(4.0));
        props.insert("Label".to_string(), ScriptValue::Text("fast".into()));
        props.insert("Enabled".to_string(), ScriptValue::Bool(false));
        props.insert("Steps".to_string(), ScriptValue::Number(7.0));
        let mut world = world(&dir);
        let entity =
            csharp_scripted_with(&mut world, "scripts/managed/Game.dll", "Game.Spin", props);
        world.run_system_once(sync_scripts).unwrap();
        advance(&mut world, 0.5);
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();

        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 2.0);
        let lines = world.resource::<ScriptLog>().lines().to_vec();
        assert!(
            lines.contains(&"props 4.0|fast|False|7".to_owned()),
            "{lines:?}"
        );

        // a reload builds a new instance, so the scene overrides are applied again
        world
            .resource_mut::<ScriptReload>()
            .request("scripts/managed/Game.dll");
        world.run_system_once(sync_scripts).unwrap();
        advance(&mut world, 0.5);
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 4.0);
        let starts = world
            .resource::<ScriptLog>()
            .lines()
            .iter()
            .filter(|line| line.as_str() == "props 4.0|fast|False|7")
            .count();
        assert_eq!(starts, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bad_override_is_logged_and_the_default_keeps_running() {
        let _guard = csharp_guard();
        if !dotnet_available() {
            return;
        }
        let dir = scratch("csharp-props-bad");
        std::fs::create_dir_all(dir.join("scripts/managed")).unwrap();
        let bindings = bindings();
        let game = build_source(&dir.join("project"), &bindings, property_script_source());
        stage_managed(&dir, &game, &bindings);

        let mut props = BTreeMap::new();
        props.insert("Speed".to_string(), ScriptValue::Text("quick".into()));
        props.insert("Steps".to_string(), ScriptValue::Text("many".into()));
        let mut world = world(&dir);
        let entity =
            csharp_scripted_with(&mut world, "scripts/managed/Game.dll", "Game.Spin", props);
        world.run_system_once(sync_scripts).unwrap();
        advance(&mut world, 0.5);
        world.run_system_once(update_scripts).unwrap();
        world.run_system_once(update_csharp_scripts).unwrap();

        assert_eq!(world.get::<Transform>(entity).unwrap().translation.x, 1.0);
        let lines = world.resource::<ScriptLog>().lines().to_vec();
        assert!(
            lines.iter().any(|line| {
                line.contains("csharp properties")
                    && line.contains("Speed")
                    && line.contains("Steps")
            }),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"props 2.0|spin|True|3".to_owned()),
            "{lines:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    fn advance(world: &mut World, seconds: f32) {
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(seconds));
    }

    /// Times the per-frame scripting pass with many scripts and objects.
    ///
    /// Run with `cargo test -p eme_script --release --lib bench_script_updates -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark"]
    fn bench_script_updates() {
        use std::io::Write;
        let scripts = 200usize;
        let objects = 4000usize;
        let frames = 300usize;
        let dir = scratch("bench");
        std::fs::write(
            dir.join("move.lua"),
            "return { on_update = function(self, dt) local x, y, z = self:get_translation() self:translate(dt, 0, 0) end }",
        )
        .unwrap();
        let mut world = world(&dir);
        for id in 0..scripts as u64 {
            let entity = world
                .spawn((Name::new(format!("Script{id}")), Transform::default()))
                .id();
            world.entity_mut(entity).insert(ScriptedObject {
                object_id: id + 1,
                script: ScriptRef {
                    path: "move.lua".into(),
                    ..Default::default()
                },
            });
        }
        for id in 0..objects {
            world.spawn((Name::new(format!("Object{id}")), Transform::default()));
        }
        world.run_system_once(sync_scripts).unwrap();
        world
            .resource_mut::<Time>()
            .advance_by(core::time::Duration::from_secs_f32(1.0 / 60.0));
        world.run_system_once(update_scripts).unwrap();

        let start = std::time::Instant::now();
        for _ in 0..frames {
            world.run_system_once(update_scripts).unwrap();
        }
        let elapsed = start.elapsed();
        let per_frame = elapsed.as_secs_f64() * 1000.0 / frames as f64;
        let _ = writeln!(
            std::io::stderr(),
            "bench: {scripts} scripts, {objects} objects, {frames} frames ->              {per_frame:.3} ms/frame, {:.1} frames/s",
            1000.0 / per_frame
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn script_files_are_listed_per_language() {
        let dir = scratch("list");
        std::fs::create_dir_all(dir.join("scripts/managed")).unwrap();
        std::fs::write(dir.join("scripts/spin.lua"), "return {}").unwrap();
        std::fs::write(dir.join("scripts/managed/Game.dll"), b"dll").unwrap();
        std::fs::write(dir.join("scripts/managed/Game.runtimeconfig.json"), b"{}").unwrap();
        std::fs::write(dir.join("scripts/managed/Fabr.Bindings.dll"), b"runtime").unwrap();
        std::fs::write(dir.join("scripts/managed/Game.resources.dll"), b"resources").unwrap();
        assert_eq!(script_files(&dir), ["scripts/spin.lua"]);
        assert_eq!(
            script_files_for(&dir, ScriptLanguage::CSharp),
            ["scripts/managed/Game.dll"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn entity_find_returns_a_usable_handle() {
        let dir = scratch("find");
        std::fs::write(
            dir.join("find.lua"),
            "return { on_update = function(self) local other = fabr.entity.find('Other'); if other then other:set_translation(9, 0, 0) end end }",
        )
        .unwrap();
        let mut world = world(&dir);
        let other = world.spawn((Name::new("Other"), Transform::default())).id();
        let _entity = scripted(&mut world, "find.lua", BTreeMap::new());
        world.run_system_once(sync_scripts).unwrap();
        world.run_system_once(update_scripts).unwrap();
        assert_eq!(world.get::<Transform>(other).unwrap().translation.x, 9.0);
    }
}
