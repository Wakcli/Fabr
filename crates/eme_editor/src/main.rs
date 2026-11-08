//! Fabr editor shell.

extern crate alloc;

use std::path::{Path, PathBuf};

use eme::asset::AssetPlugin;
use eme::feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
use eme::prelude::*;
use eme::render::settings::{RenderCreation, WgpuFeatures, WgpuSettings};
use eme::render::RenderPlugin;
use eme::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use eme::window::WindowResolution;
use eme_editor_core::{HubState, Layout, Project, ProjectId, Theme, EDITOR_VERSION};

mod app;
mod build;
mod clipboard;
mod console;
mod import;
mod mixer;
mod packages;
mod shortcuts;
mod extensions;
mod profile;
mod hud;
mod memory;
mod orbit;
mod persist;
mod play;
mod scene;
mod script_watch;
mod settings;
mod screens;
mod selection_outline;
mod theme_bridge;
mod tokens;
mod undo;

/// Product name shown in the window title, the chrome, and the hub.
pub const PRODUCT: &str = "Fabr";

const TITLE: &str = PRODUCT;
const THEME_PATH: &str = "editor/theme.ron";
pub(crate) const LAYOUT_PATH: &str = "editor/layout.ron";
/// Project file the dock layout is saved to.
pub(crate) const PROJECT_LAYOUT_PATH: &str = "fabr.layout.ron";
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

pub(crate) fn asset_path(rel: &str) -> PathBuf {
    asset_root().join(rel)
}

/// Editor hub state.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct HubRes(pub HubState);

/// Editor panel layout.
#[derive(Resource, Deref, DerefMut)]
pub struct LayoutRes(pub Layout);


#[cfg(feature = "plugins")]
#[expect(
    unsafe_code,
    reason = "external plugins are native libraries registered through a C ABI"
)]
fn load_external(
    app: &mut App,
    root: &Path,
    entries: &[(String, String)],
) -> (Vec<eme_plugin::LoadedPlugin>, Vec<String>) {
    // SAFETY: `app` is borrowed for the call and stays alive afterwards; the
    // configured libraries are the user's plugin binaries.
    unsafe { eme_plugin::load_and_register(std::ptr::from_mut(app).cast(), root, entries) }
}

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
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    features: WgpuFeatures::TIMESTAMP_QUERY
                        | WgpuFeatures::TIMESTAMP_QUERY_INSIDE_ENCODERS,
                    ..default()
                })),
                ..default()
            })
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
    .add_systems(Update, capture_on_frame);
    run_with_plugins(app);
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

/// The project's saved dock layout file.
pub(crate) fn project_layout_path() -> PathBuf {
    build::workspace_dir().join(PROJECT_LAYOUT_PATH)
}

/// Reads a layout file, returning `None` for missing or invalid files.
pub(crate) fn read_layout(path: &Path) -> Option<Layout> {
    let src = std::fs::read_to_string(path).ok()?;
    match ron::from_str::<Layout>(&src) {
        Ok(layout) if layout.validate().is_ok() => Some(layout),
        Ok(layout) => {
            warn!(
                "{} is invalid ({}); ignoring it",
                path.display(),
                layout.validate().err().unwrap_or_default()
            );
            None
        }
        Err(err) => {
            warn!("{} is malformed ({err}); ignoring it", path.display());
            None
        }
    }
}

/// Writes a layout file, creating the parent folder when needed.
pub(crate) fn write_layout(layout: &Layout, path: &Path) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(
        layout,
        ron::ser::PrettyConfig::default().struct_names(true),
    )
    .map_err(|err| format!("could not serialize layout: {err}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
}

fn load_layout() -> Layout {
    let project = project_layout_path();
    if let Some(layout) = read_layout(&project) {
        return layout;
    }
    let fallback = asset_path(LAYOUT_PATH);
    if let Some(layout) = read_layout(&fallback) {
        return layout;
    }
    warn!("no saved layout; falling back to the default editor layout");
    Layout::default_editor()
}

fn sample_hub() -> HubState {
    let mut hub = HubState::new();
    for (id, name) in [(1, PRODUCT), (2, "Lumen Fields"), (3, "Ashfall")] {
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



#[cfg(feature = "plugins")]
fn run_with_plugins(mut app: App) {
    let root = build::workspace_dir();
    let extensions = eme_game_runtime::load_for(&root);
    let entries: Vec<(String, String)> = extensions
        .external
        .iter()
        .map(|plugin| (plugin.name.clone(), plugin.library.clone()))
        .collect();
    let (plugins, errors) = load_external(&mut app, &root, &entries);
    for error in errors {
        warn!("plugin: {error}");
    }
    for plugin in &plugins {
        info!("plugin: loaded {}", plugin.name);
    }
    let _plugins = plugins;
    app.run();
}

#[cfg(not(feature = "plugins"))]
fn run_with_plugins(app: App) {
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme-layout-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn layout_files_round_trip() {
        let dir = scratch("roundtrip");
        let path = dir.join(PROJECT_LAYOUT_PATH);
        let mut layout = Layout::default_editor();
        layout.set_ratio_at(1, 0.5);
        write_layout(&layout, &path).unwrap();
        assert_eq!(read_layout(&path), Some(layout));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn malformed_layout_files_are_ignored() {
        let dir = scratch("malformed");
        let path = dir.join(PROJECT_LAYOUT_PATH);
        std::fs::write(&path, "not a layout").unwrap();
        assert_eq!(read_layout(&path), None);
        assert_eq!(read_layout(&dir.join("missing.ron")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn structurally_invalid_layouts_are_ignored() {
        let dir = scratch("invalid");
        let path = dir.join(PROJECT_LAYOUT_PATH);
        let broken = Layout::Split {
            axis: eme_editor_core::Axis::Horizontal,
            ratio: 9.0,
            first: Box::new(Layout::default_editor()),
            second: Box::new(Layout::default_editor()),
        };
        write_layout(&broken, &path).unwrap();
        assert_eq!(read_layout(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_default_layout_survives_a_round_trip() {
        let dir = scratch("default");
        let path = dir.join(PROJECT_LAYOUT_PATH);
        let layout = Layout::default_editor();
        write_layout(&layout, &path).unwrap();
        assert_eq!(read_layout(&path), Some(layout));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
