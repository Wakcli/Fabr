//! Emberrly editor shell.

use std::path::PathBuf;

use eme::asset::AssetPlugin;
use eme::feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
use eme::prelude::*;
use eme::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use eme::window::WindowResolution;
use eme_editor_core::{HubState, Layout, Project, ProjectId, Theme, EDITOR_VERSION};

mod app;
mod build;
mod clipboard;
mod orbit;
mod persist;
mod scene;
mod screens;
mod selection_outline;
mod tabs;
mod theme_bridge;
mod tokens;
mod undo;

const TITLE: &str = "Emberrly";
const THEME_PATH: &str = "editor/theme.ron";
const LAYOUT_PATH: &str = "editor/layout.ron";
/// The editor assets live in the workspace root, not in this crate, and cargo
/// points the asset server at the crate directory.
const ASSET_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Assets next to the executable win (shipped app); the dev build falls back to
/// the workspace assets. `EME_ASSET_ROOT` overrides both.
fn asset_root() -> PathBuf {
    if let Some(root) = std::env::var_os("EME_ASSET_ROOT") {
        return PathBuf::from(root);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")))
        && dir.is_dir()
    {
        return dir;
    }
    PathBuf::from(ASSET_ROOT)
}

fn asset_path(rel: &str) -> PathBuf {
    asset_root().join(rel)
}

/// Editor hub state.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct HubRes(pub HubState);

/// Editor panel layout.
#[derive(Resource, Deref, DerefMut)]
pub struct LayoutRes(pub Layout);

fn main() {
    let mut app = App::new();
    let start = std::env::var("EME_EDITOR_SCREEN").unwrap_or_else(|_| {
        if std::env::var_os("EME_EDITOR_SHOT").is_some() {
            "editor".into()
        } else {
            "hub".into()
        }
    });
    if start == "editor" {
        app.insert_resource(app::Screen::Editor);
    }
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: TITLE.into(),
                    resolution: WindowResolution::new(1600, 900),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_root().to_string_lossy().into_owned(),
                ..default()
            }),
        FeathersPlugins,
    ))
    .insert_resource(load_theme())
    .insert_resource(HubRes(sample_hub()))
    .insert_resource(LayoutRes(load_layout()))
    .add_plugins((
        app::EditorShellPlugin,
        screens::editor::EditorPlugin,
        screens::hub::HubPlugin,
    ))
    .add_systems(Startup, spawn_camera)
    .add_systems(Update, capture_on_frame)
    .run();
}

fn capture_on_frame(mut commands: Commands, mut frame: Local<u32>) {
    let Ok(path) = std::env::var("EME_EDITOR_SHOT") else {
        return;
    };
    let target: u32 = std::env::var("EME_EDITOR_SHOT_FRAME")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(30);
    *frame += 1;
    if *frame != target {
        return;
    }
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            save_to_disk(path.clone())(captured);
            exit.write(AppExit::Success);
        },
    );
    info!("capture requested at frame {target}");
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        Camera2d
        Camera { order: 2 }
    });
}

fn load_theme() -> UiTheme {
    match Theme::load_from_path(&asset_path(THEME_PATH)) {
        Ok(theme) => UiTheme(theme_bridge::props(&theme)),
        Err(err) => {
            warn!("{THEME_PATH} unavailable ({err}); falling back to the feathers dark theme");
            UiTheme(create_dark_theme())
        }
    }
}

fn load_layout() -> Layout {
    let src = match std::fs::read_to_string(asset_path(LAYOUT_PATH)) {
        Ok(src) => src,
        Err(err) => {
            warn!("{LAYOUT_PATH} unavailable ({err}); falling back to the default editor layout");
            return Layout::default_editor();
        }
    };
    match ron::from_str(&src) {
        Ok(layout) => layout,
        Err(err) => {
            warn!("{LAYOUT_PATH} is malformed ({err}); falling back to the default editor layout");
            Layout::default_editor()
        }
    }
}

fn sample_hub() -> HubState {
    let mut hub = HubState::new();
    for (id, name) in [(1, "Emberrly"), (2, "Lumen Fields"), (3, "Ashfall")] {
        let slug = name.to_lowercase().replace(' ', "-");
        hub.add_or_touch(Project {
            id: ProjectId(id),
            name: name.into(),
            root: format!("/projects/{slug}").into(),
            last_opened: Some(1_760_000_000 + id),
            engine_version: EDITOR_VERSION.into(),
        });
    }
    hub
}
