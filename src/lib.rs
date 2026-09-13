#![cfg_attr(docsrs, feature(doc_cfg))]
#![expect(
    clippy::doc_markdown,
    reason = "Clippy lints for un-backticked identifiers within the cargo features list, which we don't want."
)]
//! [![Eme Logo](https://eme.org/assets/eme_logo_docs.svg)](https://eme.org)
//!
//! Eme is an open-source, modular game engine built in Rust, with a focus on developer productivity
//! and performance.
//!
//! Check out the [Eme website](https://eme.org) for more information, read the
//! [Quick Start Guide](https://eme.org/learn/quick-start/introduction) for a step-by-step introduction, and [engage with our
//! community](https://eme.org/community/) if you have any questions or ideas!
//!
//! ## Example
//!
//! Here is a simple "Hello, World!" Eme app:
//! ```
//! use eme::prelude::*;
//!
//! fn main() {
//!    App::new()
//!        .add_systems(Update, hello_world_system)
//!        .run();
//! }
//!
//! fn hello_world_system() {
//!    println!("hello world");
//! }
//! ```
//!
//! Don't let the simplicity of the example above fool you. Eme is a [fully featured game engine](https://eme.org),
//! and it gets more powerful every day!
//!
//! ## This Crate
//!
//! The `eme` crate is a container crate that makes it easier to consume Eme subcrates.
//! The defaults provide a "full engine" experience, but you can easily enable or disable features
//! in your project's `Cargo.toml` to meet your specific needs. See Eme's `Cargo.toml` for a full
//! list of available features.
//!
//! If you prefer, you can also use the individual Eme crates directly.
//! Each module in the root of this crate, except for the prelude, can be found on crates.io
//! with `eme_` appended to the front, e.g., `app` -> [`eme_app`](https://docs.rs/eme_app/*/eme_app/).
#![doc = include_str!("../docs/cargo_features.md")]
#![doc(
    html_logo_url = "https://eme.org/assets/icon.png",
    html_favicon_url = "https://eme.org/assets/icon.png"
)]
#![no_std]

pub use eme_internal::*;

// Wasm does not support dynamic linking.
#[cfg(all(feature = "dynamic_linking", not(target_family = "wasm")))]
#[expect(
    unused_imports,
    clippy::single_component_path_imports,
    reason = "This causes Eme to be compiled as a dylib when using dynamic linking and therefore cannot be removed or changed without affecting dynamic linking."
)]
use eme_dylib;
