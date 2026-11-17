#![expect(unsafe_code, reason = "C# scripts are driven through a raw FFI callback table")]

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
use alloc::ffi::CString;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;

use alloc::sync::Arc;
use eme_ecs::prelude::*;
use eme_math::prelude::*;
use eme_script_csharp::{ABI_VERSION, FabrApi, shared_host};

use crate::{ApiState, FrameState, ScriptLog};

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

struct CSharpContext {
    /// Shared operation surface: the same queues the Lua host drains, so a
    /// callback added here reaches the engine through one apply pass.
    api: ApiState,
}

/// Loads one scene script through the managed bootstrap.
pub(crate) fn load(
    scripts: &mut CSharpScripts,
    entity: Entity,
    path: &str,
    class: &str,
    assembly: &Path,
) -> Result<CSharpInstance, String> {
    if !assembly.is_file() {
        return Err(format!("{}: no such assembly", assembly.display()));
    }
    let bindings = assembly.with_file_name(BINDINGS_ASSEMBLY);
    if !bindings.is_file() {
        return Err(format!(
            "{}: missing managed bootstrap next to the assembly",
            bindings.display()
        ));
    }
    let bootstrap = scripts.bootstrap(&bindings)?;
    let state = ApiState {
        self_entity: entity,
        ..Default::default()
    };
    let mut context = Box::new(CSharpContext { api: state });
    let mut api = FabrApi {
        version: ABI_VERSION,
        size: size_of::<FabrApi>() as u32,
        context: std::ptr::from_mut(&mut *context).cast::<c_void>(),
        log: api_log,
        set_translation: api_set_translation,
        get_translation: api_get_translation,
    };
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
        type Load = unsafe extern "C" fn(*mut FabrApi, u64, *const c_char, *const c_char) -> i32;
        type Start = unsafe extern "C" fn(*mut FabrApi, u64) -> i32;
        type Update = unsafe extern "C" fn(*mut FabrApi, u64, f32) -> i32;
        type Stop = unsafe extern "C" fn(*mut FabrApi, u64) -> i32;
        type Unload = unsafe extern "C" fn(u64) -> i32;
        // SAFETY: the managed contract fixes these signatures for the five
        // [UnmanagedCallersOnly] methods, resolved from validated exports.
        let bootstrap = unsafe {
            Bootstrap {
                load: std::mem::transmute::<*mut c_void, Load>(load),
                start: std::mem::transmute::<*mut c_void, Start>(start),
                update: std::mem::transmute::<*mut c_void, Update>(update),
                stop: std::mem::transmute::<*mut c_void, Stop>(stop),
                unload: std::mem::transmute::<*mut c_void, Unload>(unload),
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

/// # Safety
/// `context` must point at the issuing instance's context and `line` at a
/// NUL-terminated UTF-8 string.
unsafe extern "C" fn api_log(context: *mut c_void, line: *const c_char) {
    // SAFETY: the caller passes the context registered in its own table.
    let Some(context) = (unsafe { context.cast::<CSharpContext>().as_mut() }) else {
        return;
    };
    if line.is_null() {
        return;
    }
    // SAFETY: the managed side sends a null-terminated UTF-8 string.
    let text = unsafe { CStr::from_ptr(line) }.to_string_lossy().into_owned();
    context.api.log.push(text);
}

/// # Safety
/// `context` must point at the issuing instance's context.
unsafe extern "C" fn api_set_translation(
    context: *mut c_void,
    id: u64,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    // SAFETY: see `api_log`.
    let Some(context) = (unsafe { context.cast::<CSharpContext>().as_mut() }) else {
        return 0;
    };
    let entity = Entity::from_bits(id);
    if !context.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    let (_, rotation) = context.api.transform(entity);
    let translation = Vec3::new(x, y, z);
    context.api.set_transform(entity, translation, rotation);
    context.api.ops.retain(|(target, _, _)| *target != entity);
    context.api.ops.push((entity, translation, rotation));
    1
}

/// # Safety
/// `context` must point at the issuing instance's context and `out` at room
/// for three floats.
unsafe extern "C" fn api_get_translation(
    context: *mut c_void,
    id: u64,
    out: *mut f32,
) -> i32 {
    // SAFETY: see `api_log`.
    let Some(context) = (unsafe { context.cast::<CSharpContext>().as_mut() }) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    let entity = Entity::from_bits(id);
    if !context.api.frame.transforms.contains_key(&entity) {
        return 0;
    }
    let (translation, _) = context.api.transform(entity);
    // SAFETY: the managed side passes room for three floats.
    unsafe {
        *out = translation.x;
        *out.add(1) = translation.y;
        *out.add(2) = translation.z;
    }
    1
}

/// Runs one C# instance against the frame's transform snapshot.
///
/// Returns the translation writes the script requested.
pub(crate) fn step(
    instance: &mut CSharpInstance,
    frame: &Arc<FrameState>,
    dt: f32,
    log: &mut ScriptLog,
) -> Vec<(Entity, Vec3, Quat)> {
    if instance.failed {
        return Vec::new();
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
    for line in instance.context.api.log.drain(..) {
        log.push(line);
    }
    if code != 0 {
        instance.failed = true;
        log.push(format!("script {}: disabled after a managed error", instance.path));
        return Vec::new();
    }
    std::mem::take(&mut instance.context.api.ops)
}

/// Stops every loaded instance, for example when play mode ends.
pub(crate) fn stop_all(scripts: &mut CSharpScripts, log: &mut ScriptLog) {
    for (_, mut instance) in scripts.instances.drain() {
        stop(&mut instance, log);
        // SAFETY: the handle belongs to this instance and the bootstrap is live.
        unsafe { (instance.bootstrap.unload)(instance.entity.to_bits()) };
    }
}
