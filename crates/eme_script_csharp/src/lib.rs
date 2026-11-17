#![expect(unsafe_code, reason = "hosting the CLR requires raw FFI calls")]

//! Hosts the .NET runtime in-process so Fabr can run compiled C# gameplay scripts.
//!
//! The crate finds `hostfxr`, initializes a runtime from an assembly's
//! `runtimeconfig.json`, and resolves `[UnmanagedCallersOnly]` entry points from
//! that assembly. Scene integration lives in the app crates.

extern crate alloc;

use alloc::ffi::CString;
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};

type HostfxrInitialize = unsafe extern "C" fn(
    *const c_char,
    *const *const c_char,
    *mut *mut c_void,
) -> i32;
type HostfxrGetDelegate = unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> i32;
type HostfxrClose = unsafe extern "C" fn(*mut c_void);
type LoadAssemblyAndGetFunction = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_void,
    *mut *mut c_void,
) -> i32;

/// `hostfxr_delegate_type.hdt_load_assembly_and_get_function_pointer`.
const LOAD_ASSEMBLY_DELEGATE: i32 = 5;
/// `UNMANAGEDCALLERSONLY_METHOD` from `hostfxr.h`.
const UNMANAGED_CALLERS_ONLY: isize = -1;

/// Why the runtime could not be hosted.
#[derive(Debug)]
pub enum HostError {
    /// No .NET installation was found.
    MissingDotnet,
    /// The hostfxr library could not be opened.
    LoadHostfxr(String),
    /// A required hostfxr symbol is missing.
    MissingSymbol(String),
    /// Runtime initialization failed.
    Initialize(i32),
    /// The assembly entry point could not be resolved.
    EntryPoint(i32),
    /// A string could not be converted for FFI.
    Text(String),
}

impl core::fmt::Display for HostError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HostError::MissingDotnet => write!(f, "no .NET installation found"),
            HostError::LoadHostfxr(err) => write!(f, "could not load hostfxr: {err}"),
            HostError::MissingSymbol(symbol) => write!(f, "hostfxr is missing {symbol}"),
            HostError::Initialize(code) => write!(f, "hostfxr initialization failed ({code})"),
            HostError::EntryPoint(code) => write!(f, "could not resolve the entry point ({code})"),
            HostError::Text(err) => write!(f, "path is not valid UTF-8: {err}"),
        }
    }
}

/// ABI version of [`FabrApi`].
///
/// Bump on every field addition. Fields are append-only: the managed side reads
/// this first and refuses to run against a table it was not compiled for.
pub const ABI_VERSION: u32 = 2;

/// Return code `ScriptHost.Load` uses when the SDK rejects the engine ABI.
pub const ABI_MISMATCH: i32 = 2;

/// Table of host callbacks the managed side may call.
///
/// `context` is an opaque pointer owned by the caller and passed back to every
/// callback, so the engine can attach a world or a borrow without statics.
///
/// Writes queue an operation against the current frame; the scripting pass
/// applies it after the managed call returns. Reads answer from the frame the
/// script was given, which includes the script's own unapplied writes.
#[repr(C)]
pub struct FabrApi {
    /// Version of this table's layout.
    pub version: u32,
    /// Size of this table in bytes, for diagnostics on mismatch.
    pub size: u32,
    /// Opaque context passed back to every callback.
    pub context: *mut c_void,
    /// Writes one UTF-8 log line.
    pub log: unsafe extern "C" fn(*mut c_void, *const c_char),
    /// Sets an object's world translation; returns 1 on success.
    pub set_translation: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32) -> i32,
    /// Reads an object's world translation into three floats; returns 1 on success.
    pub translation: unsafe extern "C" fn(*mut c_void, u64, *mut f32) -> i32,

    /// Seconds elapsed in the last frame.
    pub delta: unsafe extern "C" fn(*mut c_void) -> f32,
    /// Seconds since the app started.
    pub elapsed: unsafe extern "C" fn(*mut c_void) -> f32,

    /// Whether a key is held, by project-settings key name.
    pub key: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    /// Whether a key went down this frame.
    pub key_down: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    /// Whether a mouse button is held: 1 left, 2 right, 3 middle.
    pub mouse: unsafe extern "C" fn(*mut c_void, u8) -> i32,
    /// Whether a project input action is held.
    pub action: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    /// Whether a project input action went down this frame.
    pub action_down: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,

    /// Finds the first object with this name; writes the handle on success.
    pub find: unsafe extern "C" fn(*mut c_void, *const c_char, *mut u64) -> i32,
    /// Writes an object's name as UTF-8 without a terminator; -1 when unnamed.
    pub name: unsafe extern "C" fn(*mut c_void, u64, *mut c_char, u32) -> i32,

    /// Adds to an object's world translation; returns 1 on success.
    pub translate: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32) -> i32,
    /// Reads an object's rotation as a quaternion in `[x, y, z, w]` order.
    pub rotation: unsafe extern "C" fn(*mut c_void, u64, *mut f32) -> i32,
    /// Writes an object's world rotation.
    pub set_rotation: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32, f32) -> i32,
    /// Turns an object around the world Y axis.
    pub rotate_y: unsafe extern "C" fn(*mut c_void, u64, f32) -> i32,

    /// Reads an object's linear velocity.
    pub velocity: unsafe extern "C" fn(*mut c_void, u64, *mut f32) -> i32,
    /// Writes an object's linear velocity.
    pub set_velocity: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32) -> i32,
    /// Whether an object stood on ground in the last physics step.
    pub grounded: unsafe extern "C" fn(*mut c_void, u64) -> i32,
    /// Reads an object's angular velocity.
    pub angular_velocity: unsafe extern "C" fn(*mut c_void, u64, *mut f32) -> i32,
    /// Writes an object's angular velocity.
    pub set_angular_velocity: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32) -> i32,
    /// Adds an angular impulse, as if the object had been pushed off-centre.
    pub add_spin: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32) -> i32,
    /// Asks a character controller to walk in an XZ direction.
    pub walk: unsafe extern "C" fn(*mut c_void, u64, f32, f32) -> i32,
    /// Asks a character controller to jump on the next step.
    pub jump: unsafe extern "C" fn(*mut c_void, u64) -> i32,

    /// Number of clips the object's model provides.
    pub animation_clip_count: unsafe extern "C" fn(*mut c_void, u64) -> u32,
    /// Writes one clip name as UTF-8 without a terminator; -1 when out of range.
    pub animation_clip: unsafe extern "C" fn(*mut c_void, u64, u32, *mut c_char, u32) -> i32,
    /// Plays a clip; returns 1 when the model provides it.
    pub play_animation: unsafe extern "C" fn(*mut c_void, u64, *const c_char) -> i32,
    /// Stops the current clip.
    pub stop_animation: unsafe extern "C" fn(*mut c_void, u64) -> i32,

    /// Writes the path between two points as `[x, y, z]` triples.
    ///
    /// Returns the number of points the path has, which may exceed `capacity`;
    /// call again with a large enough buffer to read them.
    pub find_path: unsafe extern "C" fn(*mut c_void, *const f32, *const f32, *mut f32, u32)
        -> u32,
    /// Sends a nav agent to a goal, adding the agent when the object lacks one.
    pub move_to: unsafe extern "C" fn(*mut c_void, u64, f32, f32, f32, f32) -> i32,
    /// Stops a nav agent and forgets its goal.
    pub stop_moving: unsafe extern "C" fn(*mut c_void, u64) -> i32,

    /// Queues a sound relative to the asset root; 0 sfx, 1 music.
    pub play_sound: unsafe extern "C" fn(*mut c_void, *const c_char, f32, i32) -> i32,

    /// Removes every HUD element this frame.
    pub ui_clear: unsafe extern "C" fn(*mut c_void),
    /// Draws text at normalised coordinates.
    pub ui_text:
        unsafe extern "C" fn(*mut c_void, f32, f32, f32, *const c_char, f32, f32, f32),
    /// Draws a filled panel; `rect` is `[x, y, width, height]`.
    pub ui_panel: unsafe extern "C" fn(*mut c_void, *const f32, *const f32),
    /// Draws a button; `rect` is `[x, y, width, height]`.
    pub ui_button: unsafe extern "C" fn(*mut c_void, u32, *const f32, *const c_char),
    /// Whether a button with this id was pressed since the last frame.
    pub ui_clicked: unsafe extern "C" fn(*mut c_void, u32) -> i32,

    /// Loads a scene document, replacing the current one.
    pub load_scene: unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
    /// Reloads the current scene document.
    pub reload_scene: unsafe extern "C" fn(*mut c_void) -> i32,
    /// Adds a scene document as a layer; writes the layer id on success.
    pub load_scene_additive: unsafe extern "C" fn(*mut c_void, *const c_char, *mut u64) -> i32,
    /// Removes an additive scene layer.
    pub unload_scene: unsafe extern "C" fn(*mut c_void, u64) -> i32,

    /// Springs two bodies to a rest distance.
    pub create_joint: unsafe extern "C" fn(*mut c_void, u64, u64, f32, f32, f32) -> i32,
}

/// The three lifecycle entry points a C# script class exports.
#[derive(Clone, Copy)]
pub struct ScriptEntryPoints {
    start: unsafe extern "C" fn(*mut FabrApi, u64),
    update: unsafe extern "C" fn(*mut FabrApi, u64, f32),
    stop: unsafe extern "C" fn(*mut FabrApi, u64),
}

impl ScriptEntryPoints {
    /// Calls `on_start` for `entity`.
    ///
    /// # Safety
    /// `api` must be the live callback table and `entity` a handle the managed
    /// side can use with the callbacks.
    pub unsafe fn start(&self, api: &mut FabrApi, entity: u64) {
        // SAFETY: the entry point and table come from the validated host.
        unsafe { (self.start)(api, entity) };
    }

    /// Calls `on_update` for `entity`.
    ///
    /// # Safety
    /// Same requirements as [`Self::start`].
    pub unsafe fn update(&self, api: &mut FabrApi, entity: u64, dt: f32) {
        // SAFETY: the entry point and table come from the validated host.
        unsafe { (self.update)(api, entity, dt) };
    }

    /// Calls `on_stop` for `entity`.
    ///
    /// # Safety
    /// Same requirements as [`Self::start`].
    pub unsafe fn stop(&self, api: &mut FabrApi, entity: u64) {
        // SAFETY: the entry point and table come from the validated host.
        unsafe { (self.stop)(api, entity) };
    }
}

/// Where the .NET installation lives.
pub fn dotnet_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("DOTNET_ROOT") {
        return Some(PathBuf::from(root));
    }
    let default = PathBuf::from("/usr/share/dotnet");
    default.is_dir().then_some(default)
}

/// Newest `libhostfxr` under `root`.
pub fn find_hostfxr(root: &Path) -> Option<PathBuf> {
    let fxr = root.join("host").join("fxr");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(fxr)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("libhostfxr.so"))
        .filter(|path| path.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}

/// An initialized .NET runtime and its assembly loader.
pub struct DotnetHost {
    library: Library,
    close: HostfxrClose,
    load: LoadAssemblyAndGetFunction,
    handle: *mut c_void,
}

impl DotnetHost {
    /// Starts the runtime described by `runtime_config`.
    pub fn initialize(runtime_config: &Path) -> Result<Self, HostError> {
        let root = dotnet_root().ok_or(HostError::MissingDotnet)?;
        let hostfxr = find_hostfxr(&root).ok_or(HostError::MissingDotnet)?;
        // SAFETY: the library stays loaded for the lifetime of the host, and the
        // symbols are looked up with signatures from hostfxr.h.
        let library = unsafe { Library::new(&hostfxr) }
            .map_err(|err| HostError::LoadHostfxr(err.to_string()))?;
        // SAFETY: see above; the symbol's type matches the C declaration.
        let initialize: Symbol<HostfxrInitialize> = unsafe { library.get(b"hostfxr_initialize_for_runtime_config\0") }
            .map_err(|_| HostError::MissingSymbol("hostfxr_initialize_for_runtime_config".into()))?;
        // SAFETY: same library and signature.
        let get_delegate: Symbol<HostfxrGetDelegate> = unsafe { library.get(b"hostfxr_get_runtime_delegate\0") }
            .map_err(|_| HostError::MissingSymbol("hostfxr_get_runtime_delegate".into()))?;
        // SAFETY: same library and signature.
        let close: Symbol<HostfxrClose> = unsafe { library.get(b"hostfxr_close\0") }
            .map_err(|_| HostError::MissingSymbol("hostfxr_close".into()))?;
        let initialize: HostfxrInitialize = *initialize;
        let get_delegate: HostfxrGetDelegate = *get_delegate;
        let close: HostfxrClose = *close;

        let config = CString::new(runtime_config.to_string_lossy().as_bytes())
            .map_err(|err| HostError::Text(err.to_string()))?;
        let mut handle: *mut c_void = std::ptr::null_mut();
        // SAFETY: the config path is a valid C string and `handle` is writable.
        let code = unsafe { initialize(config.as_ptr(), std::ptr::null(), &mut handle) };
        if code < 0 {
            return Err(HostError::Initialize(code));
        }
        let mut load: *mut c_void = std::ptr::null_mut();
        // SAFETY: the runtime handle is valid and `load` is writable.
        let code = unsafe { get_delegate(handle, LOAD_ASSEMBLY_DELEGATE, &mut load) };
        if code < 0 || load.is_null() {
            // SAFETY: the handle came from a successful initialization.
            unsafe { close(handle) };
            return Err(HostError::Initialize(code));
        }
        // SAFETY: the delegate was returned by hostfxr with the requested type.
        let load: LoadAssemblyAndGetFunction = unsafe { std::mem::transmute(load) };
        Ok(Self {
            library,
            close,
            load,
            handle,
        })
    }

    /// Resolves an `[UnmanagedCallersOnly]` static method.
    ///
    /// `type_name` uses the assembly-qualified form, for example
    /// `"FabrTest.Script, Host"`.
    pub fn entry_point(
        &self,
        assembly: &Path,
        type_name: &str,
        method: &str,
    ) -> Result<*mut c_void, HostError> {
        let assembly = CString::new(assembly.to_string_lossy().as_bytes())
            .map_err(|err| HostError::Text(err.to_string()))?;
        let type_name = CString::new(type_name)
            .map_err(|err| HostError::Text(err.to_string()))?;
        let method = CString::new(method).map_err(|err| HostError::Text(err.to_string()))?;
        let mut function: *mut c_void = std::ptr::null_mut();
        // SAFETY: the strings are valid, the runtime handle is live, and the
        // special delegate name selects UnmanagedCallersOnly resolution.
        let code = unsafe {
            (self.load)(
                assembly.as_ptr(),
                type_name.as_ptr(),
                method.as_ptr(),
                UNMANAGED_CALLERS_ONLY as *const c_char,
                std::ptr::null_mut(),
                &mut function,
            )
        };
        if code != 0 || function.is_null() {
            return Err(HostError::EntryPoint(code));
        }
        Ok(function)
    }

    /// Resolves the three lifecycle entry points of a C# script class.
    pub fn script(
        &self,
        assembly: &Path,
        type_name: &str,
    ) -> Result<ScriptEntryPoints, HostError> {
        let start = self.entry_point(assembly, type_name, "Start")?;
        let update = self.entry_point(assembly, type_name, "Update")?;
        let stop = self.entry_point(assembly, type_name, "Stop")?;
        type Start = unsafe extern "C" fn(*mut FabrApi, u64);
        type Update = unsafe extern "C" fn(*mut FabrApi, u64, f32);
        type Stop = unsafe extern "C" fn(*mut FabrApi, u64);
        // SAFETY: the managed contract fixes these signatures for the three
        // [UnmanagedCallersOnly] methods, resolved from validated exports.
        Ok(unsafe {
            ScriptEntryPoints {
                start: std::mem::transmute::<*mut c_void, Start>(start),
                update: std::mem::transmute::<*mut c_void, Update>(update),
                stop: std::mem::transmute::<*mut c_void, Stop>(stop),
            }
        })
    }

    /// Adds the hostfxr library to the loaded set for the process lifetime.
    pub fn keep_alive(&self) -> &Library {
        &self.library
    }
}

// SAFETY: hostfxr hosts one process-wide runtime; the engine serializes every
// managed call on the main thread, so the raw handles never cross threads in
// use even though they are stored in a shared cell.
unsafe impl Send for DotnetHost {}
// SAFETY: same as the `Send` impl above.
unsafe impl Sync for DotnetHost {}

/// The process-wide runtime, initialized from the first runtime config.
///
/// .NET supports one runtime per process; subsequent calls reuse it and ignore
/// their config. A game ships one bootstrap, so this is the intended scope.
pub fn shared_host(runtime_config: &Path) -> Result<&'static DotnetHost, HostError> {
    use std::sync::{Mutex, OnceLock};
    static HOST: OnceLock<DotnetHost> = OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());
    if let Some(host) = HOST.get() {
        return Ok(host);
    }
    let _guard = INIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(host) = HOST.get() {
        return Ok(host);
    }
    let host = DotnetHost::initialize(runtime_config)?;
    Ok(HOST.get_or_init(|| host))
}

impl Drop for DotnetHost {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful initialization and is closed once.
        unsafe { (self.close)(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn dotnet_available() -> bool {
        Command::new("dotnet")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    }

    fn build_sample(dir: &Path) -> Option<PathBuf> {
        std::fs::write(
            dir.join("Host.csproj"),
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Library</OutputType>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <EnableDynamicLoading>true</EnableDynamicLoading>
    <GenerateRuntimeConfigurationFiles>true</GenerateRuntimeConfigurationFiles>
  </PropertyGroup>
</Project>
"#,
        )
        .ok()?;
        std::fs::write(
            dir.join("Script.cs"),
            r#"using System.Runtime.InteropServices;

namespace FabrTest;

// the ABI 1 prefix of the table: a client compiled against the first layout
// must keep working against every later one
[StructLayout(LayoutKind.Sequential)]
public unsafe struct FabrApiPrefix
{
    public uint Version;
    public uint Size;
    public void* Context;
    public delegate* unmanaged[Cdecl]<void*, byte*, void> Log;
    public delegate* unmanaged[Cdecl]<void*, ulong, float, float, float, int> SetTranslation;
    public delegate* unmanaged[Cdecl]<void*, ulong, float*, int> Translation;
}

public static class Script
{
    [UnmanagedCallersOnly]
    public static int Answer() => 42;

    [UnmanagedCallersOnly]
    public static unsafe void Run(FabrApiPrefix* api)
    {
        byte[] abi = System.Text.Encoding.UTF8.GetBytes(
            $"abi {api->Version} prefix={sizeof(FabrApiPrefix)} host={api->Size}\0");
        fixed (byte* text = abi)
        {
            api->Log(api->Context, text);
        }
        byte[] message = System.Text.Encoding.UTF8.GetBytes("hello from C#\0");
        fixed (byte* text = message)
        {
            api->Log(api->Context, text);
        }
        api->SetTranslation(api->Context, 7, 1.5f, 2.0f, -3.0f);
    }

    [UnmanagedCallersOnly]
    public static unsafe void Start(FabrApiPrefix* api, ulong entity)
    {
        byte[] message = System.Text.Encoding.UTF8.GetBytes("start\0");
        fixed (byte* text = message)
        {
            api->Log(api->Context, text);
        }
        api->SetTranslation(api->Context, entity, 0.0f, 0.0f, 0.0f);
    }

    [UnmanagedCallersOnly]
    public static unsafe void Update(FabrApiPrefix* api, ulong entity, float dt)
    {
        float* position = stackalloc float[3];
        if (api->Translation(api->Context, entity, position) == 0)
        {
            return;
        }
        api->SetTranslation(api->Context, entity, position[0] + dt, position[1], position[2]);
    }

    [UnmanagedCallersOnly]
    public static unsafe void Stop(FabrApiPrefix* api, ulong entity)
    {
        byte[] message = System.Text.Encoding.UTF8.GetBytes("stop\0");
        fixed (byte* text = message)
        {
            api->Log(api->Context, text);
        }
    }
}
"#,
        )
        .ok()?;
        let status = Command::new("dotnet")
            .current_dir(dir)
            .args(["build", "-c", "Release", "-o", "out"])
            .status()
            .ok()?;
        status.success().then(|| dir.join("out"))
    }

mod stubs {
    // every slot needs a value; nothing calls these
    #![allow(clippy::missing_safety_doc, reason = "the stubs are never called")]

    use std::ffi::{c_char, c_void};

    pub unsafe extern "C" fn clock(_: *mut c_void) -> f32 {
        0.0
    }

    pub unsafe extern "C" fn query(_: *mut c_void, _: *const c_char) -> i32 {
        0
    }

    pub unsafe extern "C" fn mouse(_: *mut c_void, _: u8) -> i32 {
        0
    }

    pub unsafe extern "C" fn lookup(_: *mut c_void, _: *const c_char, _: *mut u64) -> i32 {
        0
    }

    pub unsafe extern "C" fn read_name(_: *mut c_void, _: u64, _: *mut c_char, _: u32) -> i32 {
        0
    }

    pub unsafe extern "C" fn write_vec3(_: *mut c_void, _: u64, _: f32, _: f32, _: f32) -> i32 {
        0
    }

    pub unsafe extern "C" fn read_vec3(_: *mut c_void, _: u64, _: *mut f32) -> i32 {
        0
    }

    pub unsafe extern "C" fn write_quad(_: *mut c_void, _: u64, _: f32, _: f32, _: f32, _: f32) -> i32 {
        0
    }

    pub unsafe extern "C" fn entity_angle(_: *mut c_void, _: u64, _: f32) -> i32 {
        0
    }

    pub unsafe extern "C" fn entity_flag(_: *mut c_void, _: u64) -> i32 {
        0
    }

    pub unsafe extern "C" fn walk(_: *mut c_void, _: u64, _: f32, _: f32) -> i32 {
        0
    }

    pub unsafe extern "C" fn entity_count(_: *mut c_void, _: u64) -> u32 {
        0
    }

    pub unsafe extern "C" fn read_clip(_: *mut c_void, _: u64, _: u32, _: *mut c_char, _: u32) -> i32 {
        0
    }

    pub unsafe extern "C" fn entity_text(_: *mut c_void, _: u64, _: *const c_char) -> i32 {
        0
    }

    pub unsafe extern "C" fn path(_: *mut c_void, _: *const f32, _: *const f32, _: *mut f32, _: u32) -> u32 {
        0
    }

    pub unsafe extern "C" fn sound(_: *mut c_void, _: *const c_char, _: f32, _: i32) -> i32 {
        0
    }

    pub unsafe extern "C" fn clear(_: *mut c_void) {
    }

    pub unsafe extern "C" fn ui_text(_: *mut c_void, _: f32, _: f32, _: f32, _: *const c_char, _: f32, _: f32, _: f32) {
    }

    pub unsafe extern "C" fn ui_panel(_: *mut c_void, _: *const f32, _: *const f32) {
    }

    pub unsafe extern "C" fn ui_button(_: *mut c_void, _: u32, _: *const f32, _: *const c_char) {
    }

    pub unsafe extern "C" fn ui_click(_: *mut c_void, _: u32) -> i32 {
        0
    }

    pub unsafe extern "C" fn flag(_: *mut c_void) -> i32 {
        0
    }

    pub unsafe extern "C" fn joint(_: *mut c_void, _: u64, _: u64, _: f32, _: f32, _: f32) -> i32 {
        0
    }
}
    fn stub_table(context: *mut c_void) -> FabrApi {
        FabrApi {
            version: ABI_VERSION,
            size: size_of::<FabrApi>() as u32,
            context,
            log,
            set_translation,
            translation: get_translation,
            delta: stubs::clock,
            elapsed: stubs::clock,
            key: stubs::query,
            key_down: stubs::query,
            mouse: stubs::mouse,
            action: stubs::query,
            action_down: stubs::query,
            find: stubs::lookup,
            name: stubs::read_name,
            translate: stubs::write_vec3,
            rotation: stubs::read_vec3,
            set_rotation: stubs::write_quad,
            rotate_y: stubs::entity_angle,
            velocity: stubs::read_vec3,
            set_velocity: stubs::write_vec3,
            grounded: stubs::entity_flag,
            angular_velocity: stubs::read_vec3,
            set_angular_velocity: stubs::write_vec3,
            add_spin: stubs::write_vec3,
            walk: stubs::walk,
            jump: stubs::entity_flag,
            animation_clip_count: stubs::entity_count,
            animation_clip: stubs::read_clip,
            play_animation: stubs::entity_text,
            stop_animation: stubs::entity_flag,
            find_path: stubs::path,
            move_to: stubs::write_quad,
            stop_moving: stubs::entity_flag,
            play_sound: stubs::sound,
            ui_clear: stubs::clear,
            ui_text: stubs::ui_text,
            ui_panel: stubs::ui_panel,
            ui_button: stubs::ui_button,
            ui_clicked: stubs::ui_click,
            load_scene: stubs::query,
            reload_scene: stubs::flag,
            load_scene_additive: stubs::lookup,
            unload_scene: stubs::entity_flag,
            create_joint: stubs::joint,
        }
    }

    struct TestContext {
        logs: Vec<String>,
        transforms: std::collections::HashMap<u64, [f32; 3]>,
    }

    /// # Safety
    /// `context` must point to a live `TestContext` and `message` must be NUL-terminated.
    unsafe extern "C" fn log(context: *mut c_void, message: *const c_char) {
        // SAFETY: the test passes its own context.
        let context = unsafe { &mut *context.cast::<TestContext>() };
        // SAFETY: the managed side writes a NUL-terminated UTF-8 string.
        let text = unsafe { std::ffi::CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned();
        context.logs.push(text);
    }

    /// # Safety
    /// `context` must point to a live `TestContext`.
    unsafe extern "C" fn set_translation(
        context: *mut c_void,
        id: u64,
        x: f32,
        y: f32,
        z: f32,
    ) -> i32 {
        // SAFETY: the test passes its own context.
        let context = unsafe { &mut *context.cast::<TestContext>() };
        context.transforms.insert(id, [x, y, z]);
        1
    }

    /// # Safety
    /// `context` must point to a live `TestContext` and `out` to three writable floats.
    unsafe extern "C" fn get_translation(
        context: *mut c_void,
        id: u64,
        out: *mut f32,
    ) -> i32 {
        // SAFETY: the test passes its own context.
        let context = unsafe { &*context.cast::<TestContext>() };
        let Some(position) = context.transforms.get(&id) else {
            return 0;
        };
        // SAFETY: the caller provides a buffer of three floats.
        unsafe {
            *out = position[0];
            *out.add(1) = position[1];
            *out.add(2) = position[2];
        }
        1
    }

    #[test]
    fn a_managed_method_can_be_called() {
        if !dotnet_available() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "dotnet unavailable; skipping the CLR host test"
            );
            return;
        }
        let dir = std::env::temp_dir().join(format!("fabr-csharp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let Some(out) = build_sample(&dir) else {
            panic!("dotnet build failed");
        };
        let host = DotnetHost::initialize(&out.join("Host.runtimeconfig.json"))
            .expect("runtime starts");
        let entry = host
            .entry_point(&out.join("Host.dll"), "FabrTest.Script, Host", "Answer")
            .expect("entry point resolves");
        // SAFETY: the sample exports a parameterless unmanaged-callers-only
        // method returning int, matching this signature.
        let answer: extern "C" fn() -> i32 = unsafe { std::mem::transmute(entry) };
        assert_eq!(answer(), 42);

        let mut context = TestContext {
            logs: Vec::new(),
            transforms: std::collections::HashMap::new(),
        };
        let mut api = stub_table(std::ptr::from_mut(&mut context).cast::<c_void>());
        let run_entry = host
            .entry_point(&out.join("Host.dll"), "FabrTest.Script, Host", "Run")
            .expect("run entry point resolves");
        // SAFETY: the sample exports `Run(FabrApi*)` as an unmanaged entry point.
        let run: unsafe extern "C" fn(*mut FabrApi) = unsafe { std::mem::transmute(run_entry) };
        // SAFETY: `api` holds valid callbacks and the context lives for the call.
        unsafe { run(std::ptr::from_ref(&api).cast_mut()) };
        let table = size_of::<FabrApi>();
        let abi = context.logs[0].clone();
        assert!(abi.starts_with(&format!("abi {ABI_VERSION} ")), "{abi}");
        let number = |key: &str| -> usize {
            abi.split(key)
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .and_then(|value| value.trim_end_matches('\0').parse().ok())
                .unwrap_or_default()
        };
        assert_eq!(number("host="), table, "{abi}");
        assert!(
            number("prefix=") < table,
            "the ABI 1 prefix must still fit inside the live table: {abi}"
        );
        assert_eq!(context.logs[1], "hello from C#");
        context.logs.clear();
        assert_eq!(context.transforms.get(&7), Some(&[1.5, 2.0, -3.0]));

        let script = host
            .script(&out.join("Host.dll"), "FabrTest.Script, Host")
            .expect("lifecycle entry points resolve");
        // SAFETY: the callbacks and context live for the whole sequence.
        unsafe {
            script.start(&mut api, 11);
            script.update(&mut api, 11, 0.25);
            script.update(&mut api, 11, 0.25);
            script.stop(&mut api, 11);
        }
        assert_eq!(context.logs, ["start", "stop"]);
        assert_eq!(context.transforms.get(&11), Some(&[0.5, 0.0, 0.0]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
