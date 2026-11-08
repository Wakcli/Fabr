#![allow(
    unsafe_code,
    reason = "loading native plugins inherently needs unsafe calls"
)]
//! External plugin loading: shared libraries with a versioned C entry point.
//!
//! The host passes an opaque `App` pointer to the plugin's registration
//! function. Both sides must be built against the same engine dylib
//! (`dynamic_linking`), which is what makes the pointer meaningful.

use core::ffi::{c_void, CStr};
use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};

/// Version of the plugin ABI the host speaks.
pub const ABI_VERSION: u32 = 1;
/// Symbol returning the plugin's ABI version.
pub const ABI_SYMBOL: &[u8] = b"fabr_plugin_abi\0";
/// Symbol the host calls to register the plugin's systems and resources.
pub const REGISTER_SYMBOL: &[u8] = b"fabr_plugin_register\0";

/// Why a plugin could not be loaded.
#[derive(Debug)]
pub enum PluginError {
    /// The library file could not be opened.
    Load {
        /// Library that failed to open.
        path: PathBuf,
        /// Loader's explanation.
        error: String,
    },
    /// A required symbol is missing.
    MissingSymbol {
        /// Library that lacks the symbol.
        path: PathBuf,
        /// Name of the missing symbol.
        symbol: String,
    },
    /// The plugin speaks a different ABI version.
    AbiMismatch {
        /// Library with the wrong ABI.
        path: PathBuf,
        /// Version the plugin reports.
        found: u32,
        /// Version the host implements.
        expected: u32,
    },
}

impl core::fmt::Display for PluginError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PluginError::Load { path, error } => {
                write!(f, "could not open {}: {error}", path.display())
            }
            PluginError::MissingSymbol { path, symbol } => {
                write!(f, "{} is missing the {symbol} symbol", path.display())
            }
            PluginError::AbiMismatch {
                path,
                found,
                expected,
            } => write!(
                f,
                "{} uses plugin ABI {found}, the host needs {expected}",
                path.display()
            ),
        }
    }
}

/// Checks a plugin's ABI version against the host's.
pub fn check_abi(found: u32, expected: u32) -> Result<(), (u32, u32)> {
    if found == expected {
        Ok(())
    } else {
        Err((found, expected))
    }
}

type AbiFn = unsafe extern "C" fn() -> u32;
type RegisterFn = unsafe extern "C" fn(*mut c_void);

/// One loaded plugin library, kept alive for the app's lifetime.
#[derive(Debug)]
pub struct LoadedPlugin {
    library: Library,
    /// Human-readable name from the plugin file.
    pub name: String,
}

impl LoadedPlugin {
    /// Calls the plugin's registration entry point with an opaque app pointer.
    ///
    /// # Safety
    /// `app` must be a valid pointer to the host's `App`, and the plugin must
    /// have been compiled against the same engine dylib.
    pub unsafe fn register(&self, app: *mut c_void) {
        // SAFETY: the symbol was resolved from the library kept alive by `self`,
        // and its signature is the plugin ABI's registration entry point.
        let Ok(symbol) = (unsafe { self.library.get::<RegisterFn>(REGISTER_SYMBOL) }) else {
            return;
        };
        let register = *symbol;
        // SAFETY: the caller guarantees `app` is the live host App this library
        // was compiled against.
        unsafe { register(app) };
    }
}

/// Loads a plugin library and validates its ABI version.
///
/// # Safety
/// Loading native code is inherently unsafe; the caller vouches for the library.
pub unsafe fn load(path: &Path) -> Result<LoadedPlugin, PluginError> {
    // SAFETY: loading a native library is the caller's stated intent; the file
    // name and its contents are their responsibility.
    let library = unsafe { Library::new(path) }.map_err(|error| PluginError::Load {
        path: path.to_path_buf(),
        error: error.to_string(),
    })?;
    // SAFETY: the symbol is looked up in the live library and its signature is
    // the ABI version function the plugin contract defines.
    let abi: Symbol<AbiFn> = unsafe { library.get(ABI_SYMBOL) }.map_err(|_| {
        PluginError::MissingSymbol {
            path: path.to_path_buf(),
            symbol: CStr::from_bytes_with_nul(ABI_SYMBOL)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    })?;
    // SAFETY: same live library, and the signature is the ABI registration entry point.
    if unsafe { library.get::<RegisterFn>(REGISTER_SYMBOL) }.is_err() {
        return Err(PluginError::MissingSymbol {
            path: path.to_path_buf(),
            symbol: CStr::from_bytes_with_nul(REGISTER_SYMBOL)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
    }
    // SAFETY: the signature was validated by the symbol's declared type.
    let found = unsafe { abi() };
    if let Err((found, expected)) = check_abi(found, ABI_VERSION) {
        return Err(PluginError::AbiMismatch {
            path: path.to_path_buf(),
            found,
            expected,
        });
    }
    let name = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "plugin".into());
    Ok(LoadedPlugin { library, name })
}

/// Loads every configured plugin and registers it into the host app.
///
/// Returns errors as strings; a bad plugin never stops the others. The caller
/// must keep the returned plugins alive for the app's lifetime.
///
/// # Safety
/// `app` must be a valid pointer to the host's `App`.
pub unsafe fn load_and_register(
    app: *mut c_void,
    root: &Path,
    entries: &[(String, String)],
) -> (Vec<LoadedPlugin>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    for (_, library) in entries {
        let candidate = PathBuf::from(library);
        let path = if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        };
        // SAFETY: the caller vouches for the configured libraries and the app pointer.
        match unsafe { load(&path) } {
            Ok(plugin) => {
                // SAFETY: `app` is the live host App, as required by `register`.
                unsafe { plugin.register(app) };
                loaded.push(plugin);
            }
            Err(err) => errors.push(err.to_string()),
        }
    }
    (loaded, errors)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn abi_versions_must_match_exactly() {
        assert!(check_abi(ABI_VERSION, ABI_VERSION).is_ok());
        assert_eq!(check_abi(ABI_VERSION + 1, ABI_VERSION), Err((2, 1)));
    }

    #[test]
    fn a_missing_library_reports_its_path() {
        let path = Path::new("/definitely/not/here/libmissing.so");
        // SAFETY: the path does not exist, so no native code is executed.
        let err = unsafe { load(path) }.unwrap_err();
        assert!(matches!(err, PluginError::Load { .. }), "{err}");
        assert!(err.to_string().contains("libmissing.so"));
    }

    fn compile(name: &str, source: &str) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("eme-plugin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let src = dir.join(format!("{name}.c"));
        std::fs::write(&src, source).ok()?;
        let lib = dir.join(format!("lib{name}.so"));
        let status = std::process::Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&lib)
            .arg(&src)
            .status()
            .ok()?;
        status.success().then_some(lib)
    }

    #[test]
    fn a_plugin_with_the_host_abi_loads() {
        let Some(lib) = compile(
            "good",
            "unsigned fabr_plugin_abi(void){return 1u;}\nvoid fabr_plugin_register(void *a){(void)a;}\n",
        ) else {
            let _ = writeln!(
                std::io::stderr(),
                "cc unavailable; skipping the real-library load test"
            );
            return;
        };
        // SAFETY: the library was compiled by this test from the known source above.
        let plugin = unsafe { load(&lib) }.expect("a correct plugin loads");
        assert_eq!(plugin.name, "libgood");
        std::fs::remove_dir_all(lib.parent().unwrap()).ok();
    }

    #[test]
    fn a_plugin_with_a_newer_abi_is_refused() {
        let Some(lib) = compile(
            "future",
            "unsigned fabr_plugin_abi(void){return 99u;}\nvoid fabr_plugin_register(void *a){(void)a;}\n",
        ) else {
            let _ = writeln!(
                std::io::stderr(),
                "cc unavailable; skipping the ABI mismatch test"
            );
            return;
        };
        // SAFETY: the library was compiled by this test from the known source above.
        let err = unsafe { load(&lib) }.unwrap_err();
        assert!(
            matches!(err, PluginError::AbiMismatch { found: 99, expected: 1, .. }),
            "{err}"
        );
        std::fs::remove_dir_all(lib.parent().unwrap()).ok();
    }

    #[test]
    fn loading_a_library_without_the_abi_symbol_fails() {
        // libc is present on every Linux system used by the editor.
        let path = Path::new("libc.so.6");
        // SAFETY: libc exports no plugin ABI, so loading it cannot run plugin code.
        let err = unsafe { load(path) }.unwrap_err();
        assert!(matches!(err, PluginError::MissingSymbol { .. }), "{err}");
    }
}
