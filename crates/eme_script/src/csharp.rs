#![expect(unsafe_code, reason = "C# scripts are driven through a raw FFI callback table")]
#![allow(
    clippy::missing_safety_doc,
    reason = "the callback contract is stated once, above the table"
)]

//! C# gameplay scripts hosted by the .NET runtime.
//!
//! A C# script is a compiled assembly listed in the scene. The scene stores the
//! class that derives from the SDK's `FabrScript`; the managed bootstrap loads
//! it into a collectible [`AssemblyLoadContext`], so rebuilding an assembly and
//! requesting a reload picks up the new code without restarting the game.
//!
//! [`AssemblyLoadContext`]: https://learn.microsoft.com/dotnet/core/dependency-loading/understanding-assemblyloadcontext
//!
//! Callbacks queue operations instead of touching the world directly, so the
//! managed side never aliases ECS borrows.

use std::collections::{HashMap, HashSet};
use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};

use alloc::sync::Arc;
use eme_ecs::prelude::*;
use eme_game_scene::ScriptValue;
use eme_math::prelude::*;
use eme_script_csharp::{ABI_VERSION, FabrApi, shared_host};

use crate::{ApiState, AudioBus, FrameState, ScriptLog};

/// Managed bootstrap assembly staged next to the game's scripts.
pub const BINDINGS_ASSEMBLY: &str = "Fabr.Bindings.dll";

/// `[UnmanagedCallersOnly]` entry points of the managed bootstrap.
#[derive(Clone, Copy)]
struct Bootstrap {
    load: unsafe extern "C" fn(*mut FabrApi, u64, *const c_char, *const c_char) -> i32,
    start: unsafe extern "C" fn(*mut FabrApi, u64) -> i32,
    update: unsafe extern "C" fn(*mut FabrApi, u64, f32) -> i32,
    stop: unsafe extern "C" fn(*mut FabrApi, u64) -> i32,
    unload: unsafe extern "C" fn(u64) -> i32,
    apply_properties: unsafe extern "C" fn(*mut FabrApi, u64, *const c_char) -> i32,
    inspect: unsafe extern "C" fn(*mut FabrApi, *const c_char, *const c_char, *mut u8, u32) -> i32,
}

/// C# scripts that are loaded and running.
#[derive(Resource, Default)]
pub struct CSharpScripts {
    bootstrap: Option<Bootstrap>,
    instances: HashMap<Entity, CSharpInstance>,
    failed: HashSet<(String, String)>,
}

// SAFETY: the CLR host and its managed contexts are only touched by the
// scripting systems, which the schedule serializes on the main thread.
unsafe impl Send for CSharpScripts {}
// SAFETY: same as the `Send` impl above.
unsafe impl Sync for CSharpScripts {}

pub(crate) struct CSharpInstance {
    entity: Entity,
    path: String,
    class: String,
    bootstrap: Bootstrap,
    api: FabrApi,
    context: Box<CSharpContext>,
    started: bool,
    failed: bool,
}

impl CSharpInstance {
    pub(crate) fn api_mut(&mut self) -> &mut ApiState {
        &mut self.context.api
    }
}

struct CSharpContext {
    /// Shared operation surface: the same queues the Lua host drains, so a
    /// callback added here reaches the engine through one apply pass.
    api: ApiState,
}

/// Managed bootstrap next to `assembly`, validated before any CLR call.
fn bindings_for(assembly: &Path) -> Result<PathBuf, String> {
    if !assembly.is_file() {
        let dir = assembly.parent().unwrap_or(assembly);
        return Err(format!(
            "{}: no such assembly; build a C# project into {}",
            assembly.display(),
            dir.display()
        ));
    }
    let bindings = assembly.with_file_name(BINDINGS_ASSEMBLY);
    if !bindings.is_file() {
        let dir = assembly.parent().unwrap_or(assembly);
        return Err(format!(
            "{}: missing managed bootstrap next to the assembly; rebuild into {}",
            bindings.display(),
            dir.display()
        ));
    }
    Ok(bindings)
}

/// ABI table for one instance, pointing at its context.
fn table(context: *mut c_void) -> FabrApi {
    FabrApi {
        version: ABI_VERSION,
        size: size_of::<FabrApi>() as u32,
        context,
        log: api_log,
        set_translation: api_set_translation,
        translation: api_translation,
        delta: api_delta,
        elapsed: api_elapsed,
        key: api_key,
        key_down: api_key_down,
        mouse: api_mouse,
        action: api_action,
        action_down: api_action_down,
        find: api_find,
        name: api_name,
        translate: api_translate,
        rotation: api_rotation,
        set_rotation: api_set_rotation,
        rotate_y: api_rotate_y,
        velocity: api_velocity,
        set_velocity: api_set_velocity,
        grounded: api_grounded,
        angular_velocity: api_angular_velocity,
        set_angular_velocity: api_set_angular_velocity,
        add_spin: api_add_spin,
        walk: api_walk,
        jump: api_jump,
        animation_clip_count: api_animation_clip_count,
        animation_clip: api_animation_clip,
        play_animation: api_play_animation,
        stop_animation: api_stop_animation,
        find_path: api_find_path,
        move_to: api_move_to,
        stop_moving: api_stop_moving,
        play_sound: api_play_sound,
        ui_clear: api_ui_clear,
        ui_text: api_ui_text,
        ui_panel: api_ui_panel,
        ui_button: api_ui_button,
        ui_clicked: api_ui_clicked,
        load_scene: api_load_scene,
        reload_scene: api_reload_scene,
        load_scene_additive: api_load_scene_additive,
        unload_scene: api_unload_scene,
        create_joint: api_create_joint,
    }
}

/// Loads one scene script through the managed bootstrap.
pub(crate) fn load(
    scripts: &mut CSharpScripts,
    entity: Entity,
    path: &str,
    class: &str,
    assembly: &Path,
) -> Result<CSharpInstance, String> {
    let bindings = bindings_for(assembly)?;
    let bootstrap = scripts.bootstrap(&bindings)?;
    let state = ApiState {
        self_entity: entity,
        ..Default::default()
    };
    let mut context = Box::new(CSharpContext { api: state });
    let mut api = table(std::ptr::from_mut(&mut *context).cast::<c_void>());
    let assembly_text = CString::new(assembly.to_string_lossy().as_bytes())
        .map_err(|err| err.to_string())?;
    let class_text = CString::new(class).map_err(|err| err.to_string())?;
    // SAFETY: the API table and context live in this instance, and both
    // strings are valid C strings for the duration of the call.
    let code = unsafe {
        (bootstrap.load)(
            &mut api,
            entity.to_bits(),
            assembly_text.as_ptr(),
            class_text.as_ptr(),
        )
    };
    if code != 0 {
        return Err(if code == eme_script_csharp::ABI_MISMATCH {
            format!(
                "csharp load failed: the managed SDK was built for a newer engine ABI than {ABI_VERSION}"
            )
        } else {
            failure(&mut context, "load")
        });
    }
    Ok(CSharpInstance {
        entity,
        path: path.to_owned(),
        class: class.to_owned(),
        bootstrap,
        api,
        context,
        started: false,
        failed: false,
    })
}

/// Drains the managed log into the failure message.
fn failure(context: &mut CSharpContext, phase: &str) -> String {
    let lines = std::mem::take(&mut context.api.log);
    if lines.is_empty() {
        format!("csharp {phase} failed")
    } else {
        lines.join("; ")
    }
}

impl CSharpScripts {
    fn bootstrap(&mut self, bindings: &Path) -> Result<Bootstrap, String> {
        if let Some(bootstrap) = self.bootstrap {
            return Ok(bootstrap);
        }
        let config = bindings.with_extension("runtimeconfig.json");
        if !config.is_file() {
            return Err(format!(
                "{}: missing .NET runtime config {}",
                bindings.display(),
                config.display()
            ));
        }
        let host = shared_host(&config).map_err(|err| err.to_string())?;
        let entry = |method: &str| {
            host.entry_point(bindings, "Fabr.Bindings.ScriptHost, Fabr.Bindings", method)
                .map_err(|err| format!("{method}: {err}"))
        };
        let load = entry("Load")?;
        let start = entry("Start")?;
        let update = entry("Update")?;
        let stop = entry("Stop")?;
        let unload = entry("Unload")?;
        let apply_properties = entry("ApplyProperties")?;
        let inspect = entry("Inspect")?;
        type Load = unsafe extern "C" fn(*mut FabrApi, u64, *const c_char, *const c_char) -> i32;
        type Start = unsafe extern "C" fn(*mut FabrApi, u64) -> i32;
        type Update = unsafe extern "C" fn(*mut FabrApi, u64, f32) -> i32;
        type Stop = unsafe extern "C" fn(*mut FabrApi, u64) -> i32;
        type Unload = unsafe extern "C" fn(u64) -> i32;
        type ApplyProperties = unsafe extern "C" fn(*mut FabrApi, u64, *const c_char) -> i32;
        type Inspect =
            unsafe extern "C" fn(*mut FabrApi, *const c_char, *const c_char, *mut u8, u32) -> i32;
        // SAFETY: the managed contract fixes these signatures for the seven
        // [UnmanagedCallersOnly] methods, resolved from validated exports.
        let bootstrap = unsafe {
            Bootstrap {
                load: std::mem::transmute::<*mut c_void, Load>(load),
                start: std::mem::transmute::<*mut c_void, Start>(start),
                update: std::mem::transmute::<*mut c_void, Update>(update),
                stop: std::mem::transmute::<*mut c_void, Stop>(stop),
                unload: std::mem::transmute::<*mut c_void, Unload>(unload),
                apply_properties: std::mem::transmute::<*mut c_void, ApplyProperties>(
                    apply_properties,
                ),
                inspect: std::mem::transmute::<*mut c_void, Inspect>(inspect),
            }
        };
        self.bootstrap = Some(bootstrap);
        Ok(bootstrap)
    }

    /// Whether any C# script is loaded, used to skip the per-frame pass.
    pub(crate) fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Mutable access to one loaded instance.
    pub(crate) fn instance_mut(&mut self, entity: Entity) -> Option<&mut CSharpInstance> {
        self.instances.get_mut(&entity)
    }

    /// Stops and unloads a script, dropping its assembly load context.
    pub(crate) fn remove(&mut self, entity: Entity, log: &mut ScriptLog) {
        let Some(mut instance) = self.instances.remove(&entity) else {
            return;
        };
        stop(&mut instance, log);
        // SAFETY: the handle belongs to this instance and the bootstrap is live.
        unsafe { (instance.bootstrap.unload)(instance.entity.to_bits()) };
    }

    /// Whether a script already failed and must not be retried until reload.
    pub(crate) fn has_failed(&self, path: &str, class: &str) -> bool {
        self.failed.contains(&(path.to_owned(), class.to_owned()))
    }

    /// Marks a script as broken so it is not reopened every frame.
    pub(crate) fn mark_failed(&mut self, path: &str, class: &str) {
        self.failed.insert((path.to_owned(), class.to_owned()));
    }

    /// Clears the failure mark so a reload can try again.
    pub(crate) fn clear_failed(&mut self, path: &str, class: &str) {
        self.failed.remove(&(path.to_owned(), class.to_owned()));
    }

    /// Registers a loaded instance.
    pub(crate) fn insert(&mut self, entity: Entity, instance: CSharpInstance) {
        self.instances.insert(entity, instance);
    }

    /// Entities with a live instance.
    pub(crate) fn entities(&self) -> Vec<Entity> {
        self.instances.keys().copied().collect()
    }

    /// Whether the entity runs this assembly and class.
    pub(crate) fn matches(&self, entity: Entity, path: &str, class: &str) -> bool {
        self.instances
            .get(&entity)
            .is_some_and(|instance| instance.path == path && instance.class == class)
    }

    /// `[Property]` defaults of `class`, read without starting it.
    ///
    /// The assembly is loaded into a throwaway collectible context and the
    /// class is constructed, so field initializers and constructors run but
    /// `OnStart` does not. Nothing the constructor does through the API leaves
    /// the discard context.
    pub fn inspect(
        &mut self,
        root: &Path,
        path: &str,
        class: &str,
    ) -> Result<BTreeMap<String, ScriptValue>, String> {
        let class = class.trim();
        if class.is_empty() {
            return Err("C# scripts need a class name".into());
        }
        let assembly = root.join(path);
        let bindings = bindings_for(&assembly)?;
        let bootstrap = self.bootstrap(&bindings)?;
        let mut context = Box::new(CSharpContext {
            api: ApiState::default(),
        });
        let mut api = table(std::ptr::from_mut(&mut *context).cast::<c_void>());
        let assembly_text =
            CString::new(assembly.to_string_lossy().as_bytes()).map_err(|err| err.to_string())?;
        let class_text = CString::new(class).map_err(|err| err.to_string())?;
        let mut read = |buffer: *mut u8, len: u32| {
            // SAFETY: both strings outlive the call and the buffer, when
            // present, has room for `len` bytes.
            unsafe {
                (bootstrap.inspect)(
                    &mut api,
                    assembly_text.as_ptr(),
                    class_text.as_ptr(),
                    buffer,
                    len,
                )
            }
        };
        let need = read(std::ptr::null_mut(), 0);
        if need < 0 {
            return Err(failure(&mut context, "inspect"));
        }
        if need == 0 {
            return Ok(BTreeMap::new());
        }
        let mut buf = vec![0u8; need as usize];
        let got = read(buf.as_mut_ptr(), need as u32);
        if got < 0 {
            return Err(failure(&mut context, "inspect"));
        }
        decode_properties(&buf[..got.min(need) as usize])
    }
}

/// Applies scene overrides to a freshly loaded instance.
pub(crate) fn apply_properties(
    instance: &mut CSharpInstance,
    values: &BTreeMap<String, ScriptValue>,
) -> Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }
    let mut text = String::new();
    for (name, value) in values {
        if name.contains(['\t', '\n']) {
            continue;
        }
        text.push_str(name);
        text.push('\t');
        match value {
            ScriptValue::Bool(flag) => text.push_str(if *flag { "true" } else { "false" }),
            ScriptValue::Number(number) => text.push_str(&number.to_string()),
            ScriptValue::Text(body) => {
                text.extend(
                    body.chars()
                        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c }),
                );
            }
        }
        text.push('\n');
    }
    let payload = CString::new(text).map_err(|err| err.to_string())?;
    // SAFETY: the table and context live in this instance and the payload is a
    // NUL-terminated string for the duration of the call.
    let code = unsafe {
        (instance.bootstrap.apply_properties)(
            &mut instance.api,
            instance.entity.to_bits(),
            payload.as_ptr(),
        )
    };
    if code == 0 {
        Ok(())
    } else {
        Err(failure(&mut instance.context, "properties"))
    }
}

/// Parses the `name\tkind\tvalue` lines the managed side writes.
fn decode_properties(buf: &[u8]) -> Result<BTreeMap<String, ScriptValue>, String> {
    let text = std::str::from_utf8(buf).map_err(|err| format!("properties: {err}"))?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(name), Some(kind), Some(raw)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("properties: malformed line {line:?}"));
        };
        let value = match kind {
            "bool" => ScriptValue::Bool(
                raw.parse::<bool>()
                    .map_err(|err| format!("properties: {name}: {err}"))?,
            ),
            "number" => ScriptValue::Number(
                raw.parse::<f64>()
                    .map_err(|err| format!("properties: {name}: {err}"))?,
            ),
            "text" => ScriptValue::Text(raw.to_owned()),
            other => return Err(format!("properties: {name}: unknown kind {other:?}")),
        };
        out.insert(name.to_owned(), value);
    }
    Ok(out)
}

/// Calls `OnStop` and moves the script's log lines into the engine log.
pub(crate) fn stop(instance: &mut CSharpInstance, log: &mut ScriptLog) {
    if instance.started && !instance.failed {
        // SAFETY: the table and context live in this instance, and the entry
        // point was resolved from the bootstrap by the validated host.
        unsafe { (instance.bootstrap.stop)(&mut instance.api, instance.entity.to_bits()) };
        instance.started = false;
    }
    for line in instance.context.api.log.drain(..) {
        log.push(line);
    }
}

// Safety contract for everything below, which only the managed side reaches
// through the table built in `CSharpInstance::new`:
//
// - `context` is the issuing instance's `CSharpContext`, or null.
// - `*const c_char` arguments are NUL-terminated UTF-8 that outlives the call.
// - `*mut f32` arguments have room for the number of floats the name implies.
// - out-parameters may be null, in which case the callback reports success
//   without writing.
//
// A callback that returns without a live context answers with the zero value
// so a managed script cannot take the host down by running at the wrong time.
macro_rules! api_state {
    ($context:expr) => {
        match
            // SAFETY: the contract above fixes the pointer's provenance.
            unsafe { $context.cast::<CSharpContext>().as_mut() }
        {
            Some(state) => state,
            None => return Default::default(),
        }
    };
}

/// Borrows the NUL-terminated UTF-8 string the managed side passed.
fn read_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    // SAFETY: the managed side keeps the buffer alive for the call.
    unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("")
}

/// Copies UTF-8 into a caller buffer without a terminator.
///
/// Returns the bytes written, which is the length clipped to `cap`.
fn write_str(text: &str, out: *mut c_char, cap: u32) -> i32 {
    let bytes = text.as_bytes();
    let len = bytes.len().min(cap as usize);
    if out.is_null() || len == 0 {
        return len as i32;
    }
    // SAFETY: the managed side passes room for `cap` bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), len) };
    len as i32
}

fn read_point(ptr: *const f32) -> Vec3 {
    // SAFETY: the managed side passes three floats.
    unsafe { Vec3::new(*ptr, *ptr.add(1), *ptr.add(2)) }
}

fn write_point(value: Vec3, out: *mut f32) {
    if out.is_null() {
        return;
    }
    // SAFETY: the managed side passes room for three floats.
    unsafe {
        *out = value.x;
        *out.add(1) = value.y;
        *out.add(2) = value.z;
    }
}

fn read_quad(ptr: *const f32) -> [f32; 4] {
    // SAFETY: the managed side passes four floats.
    unsafe { [*ptr, *ptr.add(1), *ptr.add(2), *ptr.add(3)] }
}

fn write_quad(value: [f32; 4], out: *mut f32) {
    if out.is_null() {
        return;
    }
    // SAFETY: the managed side passes room for four floats.
    unsafe {
        *out = value[0];
        *out.add(1) = value[1];
        *out.add(2) = value[2];
        *out.add(3) = value[3];
    }
}

fn write_u64(value: u64, out: *mut u64) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: the managed side passes room for one handle.
    unsafe { *out = value };
    true
}

unsafe extern "C" fn api_log(context: *mut c_void, line: *const c_char) {
    let state = api_state!(context);
    state.api.log.push(read_str(line).to_owned());
}

unsafe extern "C" fn api_set_translation(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    state.api.set_translation(entity, Vec3::new(x, y, z));
    1
}

unsafe extern "C" fn api_translation(context: *mut c_void, id: u64, out: *mut f32) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    write_point(state.api.translation(entity), out);
    1
}

unsafe extern "C" fn api_delta(context: *mut c_void) -> f32 {
    api_state!(context).api.frame.delta
}

unsafe extern "C" fn api_elapsed(context: *mut c_void) -> f32 {
    api_state!(context).api.frame.elapsed
}

unsafe extern "C" fn api_key(context: *mut c_void, name: *const c_char) -> i32 {
    let state = api_state!(context);
    let name = crate::normalize_key(read_str(name));
    i32::from(state.api.frame.keys.contains(&name))
}

unsafe extern "C" fn api_key_down(context: *mut c_void, name: *const c_char) -> i32 {
    let state = api_state!(context);
    let name = crate::normalize_key(read_str(name));
    i32::from(state.api.frame.keys_down.contains(&name))
}

unsafe extern "C" fn api_mouse(context: *mut c_void, button: u8) -> i32 {
    i32::from(api_state!(context).api.frame.mouse.contains(&button))
}

unsafe extern "C" fn api_action(context: *mut c_void, name: *const c_char) -> i32 {
    let state = api_state!(context);
    let held = state
        .api
        .frame
        .actions
        .get(read_str(name))
        .copied()
        .unwrap_or(false);
    i32::from(held)
}

unsafe extern "C" fn api_action_down(context: *mut c_void, name: *const c_char) -> i32 {
    let state = api_state!(context);
    let held = state
        .api
        .frame
        .actions_down
        .get(read_str(name))
        .copied()
        .unwrap_or(false);
    i32::from(held)
}

unsafe extern "C" fn api_find(context: *mut c_void, name: *const c_char, out: *mut u64) -> i32 {
    let state = api_state!(context);
    let Some(entity) = state.api.find(read_str(name)) else {
        return 0;
    };
    i32::from(write_u64(entity.to_bits(), out))
}

unsafe extern "C" fn api_name(
    context: *mut c_void,
    id: u64,
    out: *mut c_char,
    cap: u32,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    let Some((name, _)) = state
        .api
        .frame
        .names
        .iter()
        .find(|(_, target)| **target == entity)
    else {
        return -1;
    };
    write_str(name, out, cap)
}

unsafe extern "C" fn api_translate(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    state.api.translate(entity, Vec3::new(x, y, z));
    1
}

unsafe extern "C" fn api_rotation(context: *mut c_void, id: u64, out: *mut f32) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    let rotation = state.api.rotation(entity);
    write_quad([rotation.x, rotation.y, rotation.z, rotation.w], out);
    1
}

unsafe extern "C" fn api_set_rotation(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
    w: f32,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    state.api.set_rotation(entity, Quat::from_xyzw(x, y, z, w));
    1
}

unsafe extern "C" fn api_rotate_y(context: *mut c_void, id: u64, radians: f32) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    state.api.rotate_y(entity, radians);
    1
}

unsafe extern "C" fn api_velocity(context: *mut c_void, id: u64, out: *mut f32) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    let known = state.api.frame.velocities.contains_key(&entity)
        || state.api.local.velocities.contains_key(&entity);
    if !known {
        return 0;
    }
    write_point(state.api.velocity(entity), out);
    1
}

unsafe extern "C" fn api_set_velocity(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let state = api_state!(context);
    state
        .api
        .set_velocity(Entity::from_bits(id), Vec3::new(x, y, z));
    1
}

unsafe extern "C" fn api_grounded(context: *mut c_void, id: u64) -> i32 {
    i32::from(api_state!(context).api.grounded(Entity::from_bits(id)))
}

unsafe extern "C" fn api_angular_velocity(context: *mut c_void, id: u64, out: *mut f32) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    let known = state.api.frame.angular.contains_key(&entity)
        || state.api.local.angular.contains_key(&entity);
    if !known {
        return 0;
    }
    write_point(state.api.angular_velocity(entity), out);
    1
}

unsafe extern "C" fn api_set_angular_velocity(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let state = api_state!(context);
    state
        .api
        .set_angular_velocity(Entity::from_bits(id), Vec3::new(x, y, z));
    1
}

unsafe extern "C" fn api_add_spin(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let state = api_state!(context);
    state
        .api
        .add_spin(Entity::from_bits(id), Vec3::new(x, y, z));
    1
}

unsafe extern "C" fn api_walk(context: *mut c_void, id: u64, x: f32, z: f32) -> i32 {
    let state = api_state!(context);
    state.api.walk(Entity::from_bits(id), x, z);
    1
}

unsafe extern "C" fn api_jump(context: *mut c_void, id: u64) -> i32 {
    let state = api_state!(context);
    state.api.jump(Entity::from_bits(id));
    1
}

unsafe extern "C" fn api_animation_clip_count(context: *mut c_void, id: u64) -> u32 {
    let state = api_state!(context);
    state.api.animation_clips(Entity::from_bits(id)).len() as u32
}

unsafe extern "C" fn api_animation_clip(
    context: *mut c_void,
    id: u64,
    index: u32,
    out: *mut c_char,
    cap: u32,
) -> i32 {
    let state = api_state!(context);
    let clips = state.api.animation_clips(Entity::from_bits(id));
    let Some(clip) = clips.get(index as usize) else {
        return -1;
    };
    write_str(clip, out, cap)
}

unsafe extern "C" fn api_play_animation(
    context: *mut c_void,
    id: u64,
    name: *const c_char,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    i32::from(state.api.play_animation(entity, read_str(name).to_owned()))
}

unsafe extern "C" fn api_stop_animation(context: *mut c_void, id: u64) -> i32 {
    let state = api_state!(context);
    state.api.stop_animation(Entity::from_bits(id));
    1
}

unsafe extern "C" fn api_find_path(
    context: *mut c_void,
    from: *const f32,
    to: *const f32,
    out: *mut f32,
    capacity: u32,
) -> u32 {
    let state = api_state!(context);
    if from.is_null() || to.is_null() {
        return 0;
    }
    let Some(path) = state.api.find_path(read_point(from), read_point(to)) else {
        return 0;
    };
    let total = path.len() as u32;
    if out.is_null() {
        return total;
    }
    for (index, point) in path.iter().take(capacity as usize).enumerate() {
        // SAFETY: the managed side passes room for `capacity` points.
        write_point(*point, unsafe { out.add(index * 3) });
    }
    total
}

unsafe extern "C" fn api_move_to(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
    speed: f32,
) -> i32 {
    let state = api_state!(context);
    let entity = Entity::from_bits(id);
    if !state.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    state.api.move_to(entity, Vec3::new(x, y, z), speed);
    1
}

unsafe extern "C" fn api_stop_moving(context: *mut c_void, id: u64) -> i32 {
    let state = api_state!(context);
    state.api.stop_moving(Entity::from_bits(id));
    1
}

unsafe extern "C" fn api_play_sound(
    context: *mut c_void,
    path: *const c_char,
    volume: f32,
    bus: i32,
) -> i32 {
    let state = api_state!(context);
    let bus = if bus == 1 {
        AudioBus::Music
    } else {
        AudioBus::Sfx
    };
    state
        .api
        .play_sound(read_str(path).to_owned(), volume, bus);
    1
}

unsafe extern "C" fn api_ui_clear(context: *mut c_void) {
    api_state!(context).api.ui.clear();
}

unsafe extern "C" fn api_ui_text(
    context: *mut c_void,
    x: f32,
    y: f32,
    size: f32,
    text: *const c_char,
    r: f32,
    g: f32,
    b: f32,
) {
    let state = api_state!(context);
    state
        .api
        .ui_text(x, y, size, read_str(text).to_owned(), [r, g, b]);
}

unsafe extern "C" fn api_ui_panel(context: *mut c_void, rect: *const f32, color: *const f32) {
    let state = api_state!(context);
    if rect.is_null() || color.is_null() {
        return;
    }
    state.api.ui_panel(read_quad(rect), read_quad(color));
}

unsafe extern "C" fn api_ui_button(
    context: *mut c_void,
    id: u32,
    rect: *const f32,
    label: *const c_char,
) {
    let state = api_state!(context);
    if rect.is_null() {
        return;
    }
    state.api.ui_button(id, read_quad(rect), read_str(label).to_owned());
}

unsafe extern "C" fn api_ui_clicked(context: *mut c_void, id: u32) -> i32 {
    i32::from(api_state!(context).api.frame.ui_clicks.contains(&id))
}

unsafe extern "C" fn api_load_scene(context: *mut c_void, path: *const c_char) -> i32 {
    let state = api_state!(context);
    state.api.load_scene(read_str(path).to_owned());
    1
}

unsafe extern "C" fn api_reload_scene(context: *mut c_void) -> i32 {
    api_state!(context).api.reload_scene();
    1
}

unsafe extern "C" fn api_load_scene_additive(
    context: *mut c_void,
    path: *const c_char,
    out: *mut u64,
) -> i32 {
    let state = api_state!(context);
    let id = state.api.load_scene_additive(read_str(path).to_owned());
    i32::from(write_u64(id, out))
}

unsafe extern "C" fn api_unload_scene(context: *mut c_void, id: u64) -> i32 {
    api_state!(context).api.unload_scene(id);
    1
}

unsafe extern "C" fn api_create_joint(
    context: *mut c_void,
    a: u64,
    b: u64,
    rest: f32,
    stiffness: f32,
    damping: f32,
) -> i32 {
    let state = api_state!(context);
    let (a, b) = (Entity::from_bits(a), Entity::from_bits(b));
    // `apply` inserts the joint through Commands, which panics on a dead entity.
    if !state.api.frame.transforms.contains_key(&a) || !state.api.frame.transforms.contains_key(&b)
    {
        return 0;
    }
    state.api.create_joint(a, b, rest, stiffness, damping);
    1
}

/// Runs one C# instance against the frame's transform snapshot.
///
/// Queued operations stay in the instance's `ApiState` for
/// [`apply`](crate::ApplyTargets) to drain, the same way the Lua pass does.
pub(crate) fn step(instance: &mut CSharpInstance, frame: &Arc<FrameState>, dt: f32) {
    if instance.failed {
        return;
    }
    instance.context.api.reset_frame(Arc::clone(frame));
    let handle = instance.entity.to_bits();
    let mut code = 0;
    if !instance.started {
        // SAFETY: the table and context live in this instance, and the handle
        // is the scripted entity the snapshot describes.
        code = unsafe { (instance.bootstrap.start)(&mut instance.api, handle) };
        if code == 0 {
            instance.started = true;
        }
    }
    if code == 0 {
        // SAFETY: same instance-owned table and bootstrap.
        code = unsafe { (instance.bootstrap.update)(&mut instance.api, handle, dt) };
    }
    if code != 0 {
        instance.failed = true;
        instance.context.api.ops.clear();
        instance
            .context
            .api
            .log
            .push(format!("script {}: disabled after a managed error", instance.path));
    }
}

/// Stops every loaded instance, for example when play mode ends.
pub(crate) fn stop_all(scripts: &mut CSharpScripts, log: &mut ScriptLog) {
    for (_, mut instance) in scripts.instances.drain() {
        stop(&mut instance, log);
        // SAFETY: the handle belongs to this instance and the bootstrap is live.
        unsafe { (instance.bootstrap.unload)(instance.entity.to_bits()) };
    }
}
