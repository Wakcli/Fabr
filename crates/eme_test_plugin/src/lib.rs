#![allow(unsafe_code, reason = "the plugin ABI entry point must be unsafe")]

//! Test plugin: registers a startup system that logs through the host.

use core::ffi::c_void;
use std::io::Write;

use eme::prelude::*;

/// Marker resource inserted by the plugin to prove registration ran.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct TestPluginMarker;

/// Reports the plugin ABI this library implements.
#[unsafe(no_mangle)]
pub extern "C" fn fabr_plugin_abi() -> u32 {
    eme_plugin::ABI_VERSION
}

/// # Safety
/// `app` must be a valid pointer to the host's `App`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fabr_plugin_register(app: *mut c_void) {
    // SAFETY: the host passes its live App pointer, built against the same
    // plugin ABI and engine dylib this library was compiled with.
    let app = unsafe { &mut *app.cast::<App>() };
    app.insert_resource(TestPluginMarker);
    app.add_systems(Startup, || {
        let _ = writeln!(std::io::stderr(), "fabr test plugin loaded");
    });
}
