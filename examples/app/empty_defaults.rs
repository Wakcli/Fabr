//! An empty application with default plugins.

use eme::prelude::*;

fn main() {
    App::new().add_plugins(DefaultPlugins).run();
}
