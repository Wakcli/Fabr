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

    /// Adds the hostfxr library to the loaded set for the process lifetime.
    pub fn keep_alive(&self) -> &Library {
        &self.library
    }
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

public static class Script
{
    [UnmanagedCallersOnly]
    public static int Answer() => 42;
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
        std::fs::remove_dir_all(&dir).ok();
    }
}
