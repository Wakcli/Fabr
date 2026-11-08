#![allow(unsafe_code, reason = "the generated plugin entry point is unsafe")]

//! Minimal Rust game built with the Fabr SDK.

use fabr::prelude::*;

/// Inserted by the sample to prove the gameplay library ran.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SampleGameRunning;

#[derive(Default)]
struct SampleGame {
    elapsed: f32,
    reported: bool,
}

impl FabrGame for SampleGame {
    fn build(&self, app: &mut App) {
        app.insert_resource(SampleGameRunning);
    }

    fn on_start(&mut self, ctx: &mut GameContext) {
        ctx.log("fabr sample game started");
    }

    fn on_update(&mut self, ctx: &mut GameContext, dt: f32) {
        self.elapsed += dt;
        if !self.reported && self.elapsed >= 1.0 {
            self.reported = true;
            ctx.log("fabr sample game running");
        }
    }

    fn on_stop(&mut self, ctx: &mut GameContext) {
        ctx.log("fabr sample game stopped");
    }
}

fabr::game_plugin!(SampleGame);
