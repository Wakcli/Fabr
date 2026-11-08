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
use eme_game_runtime::PhysicsBody;
use eme_game_scene::{ScriptRef, ScriptValue};
use eme_input::prelude::*;
use eme_math::prelude::*;
use eme_time::Time;
use eme_transform::prelude::Transform;
use mlua::{Lua, RegistryKey, Table, Value};

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

struct ApiState {
    self_entity: Entity,
    transforms: HashMap<Entity, (Vec3, Quat)>,
    names: HashMap<String, Entity>,
    ops: Vec<(Entity, Vec3, Quat)>,
    log: Vec<String>,
    keys: HashSet<String>,
    keys_down: HashSet<String>,
    actions: HashMap<String, bool>,
    actions_down: HashMap<String, bool>,
    mouse: HashSet<u8>,
    audio: Vec<AudioRequest>,
    velocities: HashMap<Entity, Vec3>,
    grounded: HashSet<Entity>,
    velocity_ops: Vec<(Entity, Vec3)>,
    delta: f32,
    elapsed: f32,
    properties: BTreeMap<String, ScriptValue>,
}

impl Default for ApiState {
    fn default() -> Self {
        Self {
            self_entity: Entity::PLACEHOLDER,
            transforms: HashMap::new(),
            names: HashMap::new(),
            ops: Vec::new(),
            log: Vec::new(),
            keys: HashSet::new(),
            keys_down: HashSet::new(),
            actions: HashMap::new(),
            actions_down: HashMap::new(),
            mouse: HashSet::new(),
            audio: Vec::new(),
            velocities: HashMap::new(),
            grounded: HashSet::new(),
            velocity_ops: Vec::new(),
            delta: 0.0,
            elapsed: 0.0,
            properties: BTreeMap::new(),
        }
    }
}

/// Lua VM plus one instance per scripted entity.
#[derive(Resource)]
pub struct ScriptRuntime {
    lua: Lua,
    instances: HashMap<Entity, Instance>,
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self {
            lua: Lua::new(),
            instances: HashMap::new(),
        }
    }
}

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
        names: &HashMap<String, Entity>,
        transforms: &HashMap<Entity, (Vec3, Quat)>,
    ) -> mlua::Result<(Arc<Mutex<ApiState>>, Table)> {
        let api = Arc::new(Mutex::new(ApiState {
            self_entity: entity,
            transforms: transforms.clone(),
            names: names.clone(),
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
            let (t, _) = state.transforms.get(&state.self_entity).copied().unwrap_or_default();
            Ok((t.x, t.y, t.z))
        })?)?;
        let api_set = api.clone();
        self_table.set("set_translation", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
            let mut state = api_set.lock().unwrap();
            let entity = state.self_entity;
            let (_, r) = state.transforms.get(&entity).copied().unwrap_or_default();
            state.transforms.insert(entity, (Vec3::new(x, y, z), r));
            state.ops.retain(|(e, _, _)| *e != entity);
            state.ops.push((entity, Vec3::new(x, y, z), r));
            Ok(())
        })?)?;
        let api_move = api.clone();
        self_table.set("translate", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
            let mut state = api_move.lock().unwrap();
            let entity = state.self_entity;
            let (t, r) = state.transforms.get(&entity).copied().unwrap_or_default();
            let next = t + Vec3::new(x, y, z);
            state.transforms.insert(entity, (next, r));
            state.ops.retain(|(e, _, _)| *e != entity);
            state.ops.push((entity, next, r));
            Ok(())
        })?)?;
        let api_rotate = api.clone();
        self_table.set("rotate_y", lua.create_function(move |_, (_self, radians): (Value, f32)| {
            let mut state = api_rotate.lock().unwrap();
            let entity = state.self_entity;
            let (t, r) = state.transforms.get(&entity).copied().unwrap_or_default();
            let next = Quat::from_rotation_y(radians) * r;
            state.transforms.insert(entity, (t, next));
            state.ops.retain(|(e, _, _)| *e != entity);
            state.ops.push((entity, t, next));
            Ok(())
        })?)?;

        let api_vel = api.clone();
        self_table.set("get_velocity", lua.create_function(move |_, _self: Value| {
            let state = api_vel.lock().unwrap();
            let velocity = state
                .velocities
                .get(&state.self_entity)
                .copied()
                .unwrap_or_default();
            Ok((velocity.x, velocity.y, velocity.z))
        })?)?;
        let api_grounded = api.clone();
        self_table.set("is_grounded", lua.create_function(move |_, _self: Value| {
            let state = api_grounded.lock().unwrap();
            Ok(state.grounded.contains(&state.self_entity))
        })?)?;
        let api_set_vel = api.clone();
        self_table.set(
            "set_velocity",
            lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
                let mut state = api_set_vel.lock().unwrap();
                let entity = state.self_entity;
                let velocity = Vec3::new(x, y, z);
                state.velocities.insert(entity, velocity);
                state.grounded.remove(&entity);
                state.velocity_ops.retain(|(target, _)| *target != entity);
                state.velocity_ops.push((entity, velocity));
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
            table.set("delta", lua.create_function(move |_, ()| Ok(api.lock().unwrap().delta))?)?;
            let api = api_delta;
            table.set("elapsed", lua.create_function(move |_, ()| Ok(api.lock().unwrap().elapsed))?)?;
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
                    let volume = volume.unwrap_or(1.0);
                    let volume = if volume.is_finite() {
                        volume.clamp(0.0, 4.0)
                    } else {
                        1.0
                    };
                    let bus = bus.as_deref().map(AudioBus::parse).unwrap_or_default();
                    api_play
                        .lock()
                        .unwrap()
                        .audio
                        .push(AudioRequest { path, volume, bus });
                    Ok(())
                },
            )?,
        )?;
        input.set("key", lua.create_function(move |_, name: String| {
            Ok(api_key.lock().unwrap().keys.contains(&normalize_key(&name)))
        })?)?;
        input.set("key_down", lua.create_function(move |_, name: String| {
            Ok(api_down.lock().unwrap().keys_down.contains(&normalize_key(&name)))
        })?)?;
        input.set("mouse_pressed", lua.create_function(move |_, button: u8| {
            Ok(api_mouse.lock().unwrap().mouse.contains(&button))
        })?)?;
        let api_action = api.clone();
        input.set("action", lua.create_function(move |_, name: String| {
            Ok(api_action.lock().unwrap().actions.get(&name).copied().unwrap_or(false))
        })?)?;
        let api_action_down = api.clone();
        input.set("action_down", lua.create_function(move |_, name: String| {
            Ok(api_action_down
                .lock()
                .unwrap()
                .actions_down
                .get(&name)
                .copied()
                .unwrap_or(false))
        })?)?;
        fabr.set("input", input)?;
        fabr.set("audio", audio)?;

        let api_find = api.clone();
        let find_names = names.clone();
        fabr.set("entity", lua.create_table()?)?;
        if let Ok(table) = fabr.get::<Table>("entity") {
            table.set("find", lua.create_function(move |lua, name: String| {
                let entity = find_names.get(&name).copied();
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
        let (t, _) = state.transforms.get(&entity).copied().unwrap_or_default();
        Ok((t.x, t.y, t.z))
    })?)?;
    let set = api.clone();
    table.set("set_translation", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
        let mut state = set.lock().unwrap();
        let (_, r) = state.transforms.get(&entity).copied().unwrap_or_default();
        state.transforms.insert(entity, (Vec3::new(x, y, z), r));
        state.ops.retain(|(e, _, _)| *e != entity);
        state.ops.push((entity, Vec3::new(x, y, z), r));
        Ok(())
    })?)?;
    let translate = api.clone();
    table.set("translate", lua.create_function(move |_, (_self, x, y, z): (Value, f32, f32, f32)| {
        let mut state = translate.lock().unwrap();
        let (t, r) = state.transforms.get(&entity).copied().unwrap_or_default();
        let next = t + Vec3::new(x, y, z);
        state.transforms.insert(entity, (next, r));
        state.ops.retain(|(e, _, _)| *e != entity);
        state.ops.push((entity, next, r));
        Ok(())
    })?)?;
    let rotate = api;
    table.set("rotate_y", lua.create_function(move |_, (_self, radians): (Value, f32)| {
        let mut state = rotate.lock().unwrap();
        let (t, r) = state.transforms.get(&entity).copied().unwrap_or_default();
        let next = Quat::from_rotation_y(radians) * r;
        state.transforms.insert(entity, (t, next));
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
    let mut out = Vec::new();
    collect_scripts(&root.join("scripts"), root, 0, &mut out);
    out.sort();
    out
}

fn collect_scripts(dir: &Path, root: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scripts(&path, root, depth + 1, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lua"))
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
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

struct SceneContext<'a> {
    names: &'a HashMap<String, Entity>,
    transforms: &'a HashMap<Entity, (Vec3, Quat)>,
}

fn build_instance(
    lua: &Lua,
    entity: Entity,
    scripted: &ScriptedObject,
    full: &Path,
    src: &str,
    name: &str,
    scene: &SceneContext,
) -> Result<Instance, String> {
    let path = scripted.script.path.as_str();
    let preview =
        ScriptRuntime::eval_script(lua, full, src, None).map_err(|err| err.to_string())?;
    let mut properties = properties_from(&preview).map_err(|err| err.to_string())?;
    for (key, value) in &scripted.script.properties {
        properties.insert(key.clone(), value.clone());
    }
    let (api, self_table) = ScriptRuntime::build_api(
        lua,
        entity,
        name,
        properties,
        scene.names,
        scene.transforms,
    )
    .map_err(|err| err.to_string())?;
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
    scripted: Query<(Entity, &ScriptedObject)>,
    transforms: Query<(Entity, &Transform)>,
    names: Query<(Entity, &Name)>,
) {
    let ScriptRuntime { lua, instances } = &mut *runtime;
    if !active.0 {
        for (_, mut instance) in instances.drain() {
            stop_instance(lua, &mut instance, &mut log);
        }
        return;
    }

    let mut live = HashMap::new();
    for (entity, scripted) in &scripted {
        live.insert(entity, scripted.script.path.clone());
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

    let reloads: Vec<String> = std::mem::take(&mut reload.paths);

    let mut scene_transforms = HashMap::new();
    for (entity, transform) in &transforms {
        scene_transforms.insert(entity, (transform.translation, transform.rotation));
    }
    let mut name_map = HashMap::new();
    let mut entity_names = HashMap::new();
    for (entity, name) in &names {
        name_map.insert(name.as_str().to_owned(), entity);
        entity_names.insert(entity, name.as_str().to_owned());
    }
    let scene = SceneContext {
        names: &name_map,
        transforms: &scene_transforms,
    };

    for (entity, scripted) in &scripted {
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
        match build_instance(lua, entity, scripted, &full, &src, name, &scene) {
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
}

/// Runs `on_start` once and `on_update` for every live instance.
pub fn update_scripts(
    active: Res<ScriptsActive>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    actions: Res<InputActions>,
    root: Res<ScriptRoot>,
    mut audio: ResMut<ScriptAudio>,
    mut runtime: ResMut<ScriptRuntime>,
    mut log: ResMut<ScriptLog>,
    mut transforms: Query<(Entity, &mut Transform)>,
    mut bodies: Query<(Entity, &mut PhysicsBody)>,
    names: Query<(Entity, &Name)>,
) {
    if !active.0 {
        return;
    }
    let ScriptRuntime { lua, instances } = &mut *runtime;
    if instances.is_empty() {
        return;
    }
    let mut order: Vec<(u64, Entity)> = instances
        .iter()
        .map(|(entity, instance)| (instance.object_id, *entity))
        .collect();
    order.sort_unstable();

    let mut scene_transforms = HashMap::new();
    let mut name_map = HashMap::new();
    for (entity, transform) in &mut transforms {
        scene_transforms.insert(entity, (transform.translation, transform.rotation));
    }
    for (entity, name) in &names {
        name_map.insert(name.as_str().to_owned(), entity);
    }
    let mut velocities = HashMap::new();
    let mut grounded = HashSet::new();
    for (entity, body) in &bodies {
        velocities.insert(entity, body.velocity);
        if body.grounded {
            grounded.insert(entity);
        }
    }

    let pressed: HashSet<String> = keys
        .get_pressed()
        .map(|key| format!("{key:?}").to_ascii_lowercase())
        .map(|key| key.strip_prefix("key").map_or(key.clone(), str::to_owned))
        .collect();
    let down: HashSet<String> = keys
        .get_just_pressed()
        .map(|key| format!("{key:?}").to_ascii_lowercase())
        .map(|key| key.strip_prefix("key").map_or(key.clone(), str::to_owned))
        .collect();
    let mut action_state = HashMap::new();
    let mut action_down = HashMap::new();
    for (name, bindings) in &actions.map {
        action_state.insert(name.clone(), bindings.iter().any(|key| keys.pressed(*key)));
        action_down.insert(name.clone(), bindings.iter().any(|key| keys.just_pressed(*key)));
    }
    let mouse_pressed: HashSet<u8> = [
        (MouseButton::Left, 1u8),
        (MouseButton::Right, 2),
        (MouseButton::Middle, 3),
    ]
    .into_iter()
    .filter(|(button, _)| mouse.pressed(*button))
    .map(|(_, id)| id)
    .collect();

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
        {
            let mut api = instance.api.lock().unwrap();
            api.transforms = scene_transforms.clone();
            api.names = name_map.clone();
            api.delta = time.delta_secs();
            api.elapsed = time.elapsed_secs();
            api.keys = pressed.clone();
            api.keys_down = down.clone();
            api.actions = action_state.clone();
            api.actions_down = action_down.clone();
            api.mouse = mouse_pressed.clone();
            api.velocities = velocities.clone();
            api.grounded = grounded.clone();
            api.ops.clear();
            api.velocity_ops.clear();
        }

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
            result = update.call::<()>((self_table.clone(), time.delta_secs()));
        }
        if let Err(err) = result {
            log.push(format!("script {}: {err}", instance.path));
            instance.failed = true;
            continue;
        }

        let mut api = instance.api.lock().unwrap();
        for line in api.log.drain(..) {
            log.push(line);
        }
        for (target, velocity) in api.velocity_ops.drain(..) {
            if let Ok((_, mut body)) = bodies.get_mut(target) {
                body.velocity = velocity;
                if velocity.y > 0.0 {
                    body.grounded = false;
                }
            }
        }
        for request in api.audio.drain(..) {
            if root.0.join(&request.path).is_file() {
                log.push(format!(
                    "script audio: play {} @ {:.2} ({})",
                    request.path,
                    request.volume,
                    request.bus.label()
                ));
                audio.requests.push(request);
            } else {
                log.push(format!("script audio: missing asset {}", request.path));
            }
        }
        for (target, translation, rotation) in &api.ops {
            if let Ok((_, mut transform)) = transforms.get_mut(*target) {
                transform.translation = *translation;
                transform.rotation = *rotation;
            }
        }
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
            .init_resource::<AudioMix>()
            .init_resource::<ScriptRuntime>()
            .add_systems(Update, sync_scripts)
            .add_systems(
                Update,
                update_scripts.after(eme_game_runtime::run_behaviours),
            )
            .add_systems(Update, play_script_audio.after(update_scripts));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_ecs::system::RunSystemOnce;

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
        world.insert_resource(AudioMix::default());
        world.insert_resource(ScriptRuntime::default());
        world.insert_resource(Time::<()>::default());
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(ButtonInput::<MouseButton>::default());
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
