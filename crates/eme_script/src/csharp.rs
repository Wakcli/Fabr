#![expect(unsafe_code, reason = "C# scripts are driven through a raw FFI callback table")]

//! C# gameplay scripts hosted by the .NET runtime.
//!
//! A C# script is a compiled assembly listed in the scene; the engine resolves
//! the `Start`, `Update` and `Stop` entry points of its class and calls them
//! with an [`FabrApi`] table whose context is this script's instance state.
//! Callbacks queue operations instead of touching the world directly, so the
//! managed side never aliases ECS borrows.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, c_char, c_void};
use alloc::sync::Arc;
use std::path::Path;

use eme_ecs::prelude::*;
use eme_math::prelude::*;
use eme_script_csharp::{DotnetHost, FabrApi, ScriptEntryPoints};
use eme_transform::prelude::Transform;

use crate::ScriptLog;

/// C# scripts that are loaded and running.
#[derive(Resource, Default)]
pub struct CSharpScripts {
    host: Option<DotnetHost>,
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
    entry: ScriptEntryPoints,
    api: FabrApi,
    context: Box<CSharpContext>,
    started: bool,
    failed: bool,
}

struct CSharpContext {
    snapshot: Arc<HashMap<Entity, Vec3>>,
    ops: Vec<(Entity, Vec3)>,
    log: Vec<String>,
}

/// Resolves the `Start`/`Update`/`Stop` entry points of one scene script.
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
    let config = assembly.with_extension("runtimeconfig.json");
    if !config.is_file() {
        return Err(format!(
            "{}: missing .NET runtime config {}",
            assembly.display(),
            config.display()
        ));
    }
    if scripts.host.is_none() {
        let host = DotnetHost::initialize(&config).map_err(|err| err.to_string())?;
        scripts.host = Some(host);
    }
    let host = scripts.host.as_ref().expect("host initialized above");
    let qualified = qualified_class(class, assembly);
    let entry = host
        .script(assembly, &qualified)
        .map_err(|err| format!("{qualified}: {err}"))?;
    let mut context = Box::new(CSharpContext {
        snapshot: Arc::new(HashMap::new()),
        ops: Vec::new(),
        log: Vec::new(),
    });
    let context_ptr = std::ptr::from_mut(&mut *context).cast::<c_void>();
    Ok(CSharpInstance {
        entity,
        path: path.to_owned(),
        class: class.to_owned(),
        entry,
        api: FabrApi {
            context: context_ptr,
            log: api_log,
            set_translation: api_set_translation,
            get_translation: api_get_translation,
        },
        context,
        started: false,
        failed: false,
    })
}

/// Adds the assembly name when the author wrote only the class.
fn qualified_class(class: &str, assembly: &Path) -> String {
    if class.contains(',') {
        return class.to_owned();
    }
    let stem = assembly
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{class}, {stem}")
}

/// Calls `Stop` and moves the script's log lines into the engine log.
pub(crate) fn stop(instance: &mut CSharpInstance, log: &mut ScriptLog) {
    if instance.started && !instance.failed {
        // SAFETY: the table and context live in this instance, and the entry
        // point was resolved from the same assembly by the validated host.
        unsafe { instance.entry.stop(&mut instance.api, instance.entity.to_bits()) };
        instance.started = false;
    }
    for line in instance.context.log.drain(..) {
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
    context.log.push(text);
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
    if !context.snapshot.contains_key(&entity) {
        return 0;
    }
    context.ops.push((entity, Vec3::new(x, y, z)));
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
    let Some(position) = context.snapshot.get(&Entity::from_bits(id)) else {
        return 0;
    };
    // SAFETY: the managed side passes room for three floats.
    unsafe {
        *out = position.x;
        *out.add(1) = position.y;
        *out.add(2) = position.z;
    }
    1
}

/// Runs one C# instance against the frame's transform snapshot.
///
/// Returns the translation writes the script requested.
pub(crate) fn step(
    instance: &mut CSharpInstance,
    snapshot: &Arc<HashMap<Entity, Vec3>>,
    dt: f32,
    log: &mut ScriptLog,
) -> Vec<(Entity, Vec3)> {
    if instance.failed {
        return Vec::new();
    }
    instance.context.snapshot = Arc::clone(snapshot);
    instance.context.ops.clear();
    let entry = instance.entry;
    let handle = instance.entity.to_bits();
    if !instance.started {
        // SAFETY: the table and context live in this instance, and the handle
        // is the scripted entity the snapshot describes.
        unsafe { entry.start(&mut instance.api, handle) };
        instance.started = true;
    }
    // SAFETY: same instance-owned table and entry points.
    unsafe { entry.update(&mut instance.api, handle, dt) };
    for line in instance.context.log.drain(..) {
        log.push(line);
    }
    std::mem::take(&mut instance.context.ops)
}

/// Writes queued translations to the transform store.
pub(crate) fn apply_ops(
    ops: Vec<(Entity, Vec3)>,
    transforms: &mut Query<(Entity, &mut Transform, Option<&Name>)>,
) {
    for (target, translation) in ops {
        if let Ok((_, mut transform, _)) = transforms.get_mut(target) {
            transform.translation = translation;
        }
    }
}

/// Stops every loaded instance, for example when play mode ends.
pub(crate) fn stop_all(scripts: &mut CSharpScripts, log: &mut ScriptLog) {
    for (_, mut instance) in scripts.instances.drain() {
        stop(&mut instance, log);
    }
}

impl CSharpScripts {
    /// Whether any C# script is loaded, used to skip the per-frame pass.
    pub(crate) fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Mutable access to one loaded instance.
    pub(crate) fn instance_mut(&mut self, entity: Entity) -> Option<&mut CSharpInstance> {
        self.instances.get_mut(&entity)
    }

    /// Removes a loaded instance and calls `Stop`.
    pub(crate) fn remove(&mut self, entity: Entity, log: &mut ScriptLog) -> Option<CSharpInstance> {
        let mut instance = self.instances.remove(&entity)?;
        stop(&mut instance, log);
        Some(instance)
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

    /// The assembly path and class of a loaded instance.
    pub(crate) fn matches(&self, entity: Entity, path: &str, class: &str) -> bool {
        self.instances
            .get(&entity)
            .is_some_and(|instance| instance.path == path && instance.class == class)
    }
}
