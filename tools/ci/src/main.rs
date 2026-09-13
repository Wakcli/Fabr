//! CI script used for Eme.

mod args;
mod ci;
mod commands;
mod prepare;

pub use self::{ci::*, prepare::*};

fn main() {
    argh::from_env::<CI>().run();
}
