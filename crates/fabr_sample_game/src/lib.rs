#![allow(unsafe_code, reason = "the generated plugin entry point is unsafe")]

//! Minimal Rust game built with the Fabr SDK.

use fabr::prelude::*;

/// Inserted by the sample to prove the gameplay library ran.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SampleGameRunning;

#[derive(Default)]
struct SampleGame;

impl FabrGame for SampleGame {
    fn build(&self, app: &mut App) {
        app.insert_resource(SampleGameRunning);
        app.add_systems(Startup, || info!("fabr sample game started"));
    }
}

fabr::game_plugin!(SampleGame);
