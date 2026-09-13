//! A test to confirm that `eme` allows disabling the prepass of the standard material.
//! This is run in CI to ensure that this doesn't regress again.
use eme::{pbr::PbrPlugin, prelude::*};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(PbrPlugin {
            prepass_enabled: false,
            ..default()
        }))
        .run();
}
