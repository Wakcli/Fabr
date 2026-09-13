#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![doc(
    html_logo_url = "https://eme.org/assets/icon.png",
    html_favicon_url = "https://eme.org/assets/icon.png"
)]
#![no_std]

//! This module is separated into its own crate to enable simple dynamic linking for Eme, and should not be used directly

/// `use eme::prelude::*;` to import common components, bundles, and plugins.
pub mod prelude;

mod default_plugins;
pub use default_plugins::*;

#[cfg(feature = "eme_window")]
pub use eme_a11y as a11y;
#[cfg(target_os = "android")]
pub use eme_android as android;
#[cfg(feature = "eme_animation")]
pub use eme_animation as animation;
#[cfg(feature = "eme_anti_alias")]
pub use eme_anti_alias as anti_alias;
pub use eme_app as app;
#[cfg(feature = "eme_asset")]
pub use eme_asset as asset;
#[cfg(feature = "eme_audio")]
pub use eme_audio as audio;
#[cfg(feature = "eme_camera")]
pub use eme_camera as camera;
#[cfg(feature = "eme_camera_controller")]
pub use eme_camera_controller as camera_controller;
#[cfg(feature = "eme_clipboard")]
pub use eme_clipboard as clipboard;
#[cfg(feature = "eme_color")]
pub use eme_color as color;
#[cfg(feature = "eme_core_pipeline")]
pub use eme_core_pipeline as core_pipeline;
#[cfg(feature = "eme_curve")]
pub use eme_curve as curve;
#[cfg(feature = "eme_dev_tools")]
pub use eme_dev_tools as dev_tools;
pub use eme_diagnostic as diagnostic;
pub use eme_ecs as ecs;
#[cfg(feature = "eme_extract")]
pub use eme_extract as extract;
#[cfg(feature = "eme_feathers")]
pub use eme_feathers as feathers;
#[cfg(feature = "eme_gilrs")]
pub use eme_gilrs as gilrs;
#[cfg(feature = "eme_gizmos")]
pub use eme_gizmos as gizmos;
#[cfg(feature = "eme_gizmos_render")]
pub use eme_gizmos_render as gizmos_render;
#[cfg(feature = "eme_gltf")]
pub use eme_gltf as gltf;
#[cfg(feature = "eme_image")]
pub use eme_image as image;
pub use eme_input as input;
#[cfg(feature = "eme_input_focus")]
pub use eme_input_focus as input_focus;
#[cfg(feature = "eme_light")]
pub use eme_light as light;
#[cfg(feature = "eme_log")]
pub use eme_log as log;
#[cfg(feature = "eme_material")]
pub use eme_material as material;
pub use eme_math as math;
#[cfg(feature = "eme_mesh")]
pub use eme_mesh as mesh;
#[cfg(feature = "eme_pbr")]
pub use eme_pbr as pbr;
#[cfg(feature = "eme_picking")]
pub use eme_picking as picking;
pub use eme_platform as platform;
#[cfg(feature = "eme_post_process")]
pub use eme_post_process as post_process;
pub use eme_ptr as ptr;
pub use eme_reflect as reflect;
#[cfg(feature = "eme_remote")]
pub use eme_remote as remote;
#[cfg(feature = "eme_render")]
pub use eme_render as render;
#[cfg(feature = "eme_scene")]
pub use eme_scene as scene;
#[cfg(feature = "eme_settings")]
pub use eme_settings as settings;
#[cfg(feature = "eme_shader")]
pub use eme_shader as shader;
pub use eme_shape as shape;
#[cfg(feature = "eme_solari")]
pub use eme_solari as solari;
#[cfg(feature = "eme_sprite")]
pub use eme_sprite as sprite;
#[cfg(feature = "eme_sprite_render")]
pub use eme_sprite_render as sprite_render;
#[cfg(feature = "eme_state")]
pub use eme_state as state;
pub use eme_tasks as tasks;
#[cfg(feature = "eme_text")]
pub use eme_text as text;
pub use eme_time as time;
pub use eme_transform as transform;
#[cfg(feature = "eme_ui")]
pub use eme_ui as ui;
#[cfg(feature = "eme_ui_render")]
pub use eme_ui_render as ui_render;
#[cfg(feature = "eme_ui_widgets")]
pub use eme_ui_widgets as ui_widgets;
pub use eme_utils as utils;
#[cfg(feature = "eme_window")]
pub use eme_window as window;
#[cfg(feature = "eme_winit")]
pub use eme_winit as winit;
#[cfg(feature = "eme_world_serialization")]
pub use eme_world_serialization as world_serialization;
