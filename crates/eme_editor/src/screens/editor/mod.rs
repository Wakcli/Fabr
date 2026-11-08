//! The editor frame: a docked layout, monochrome chrome, and a perspective viewport.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::text::{FontSize, FontSourceTemplate, FontWeight};
use eme::ui_widgets::Button;
use eme::prelude::*;
use eme::scene::{bsn, Scene};
use eme_editor_core::{Layout, EDITOR_VERSION};

use crate::tokens;

mod build_dialog;
pub(crate) mod chrome;
pub(crate) mod dock;
pub(crate) mod docks;
pub(crate) mod files;
mod gizmo;
pub(crate) mod icons;
mod inspector;
mod menu;
pub(crate) mod palette;
pub(crate) mod tabs;
mod viewport;

use crate::build;
use crate::clipboard;
use crate::orbit;
use crate::persist;
use crate::scene;
use crate::undo;
use chrome::*;

/// Editor frame scene: the top bar over a dock tree built from `layout`.
pub fn screen(layout: Layout) -> impl Scene {
    let bar = top_bar();
    let tree = dock::tree(&layout);

    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ThemeBackgroundColor(tokens::WINDOW)
        Children [
            {bar}
            --
            {tree}
            --
            {dock::overlay()}
            --
            {dock::insert_caret()}
            --
            {dock::drag_ghost()}
            --
            {crate::import::panel()}
            --
            {menu::top_menu()}
            --
            {crate::settings::panel()}
            --
            {crate::mixer::panel()}
            --
            {crate::packages::panel()}
            --
            {crate::shortcuts::panel()}
            --
            {crate::extensions::panel()}
            --
            {crate::animation::panel()}
            --
            {crate::layouts::panel()}
            --
            {palette::overlay()}
            --
            {inspector::script_picker()}
            --
            {files::import_menu()}
            --
            {crate::play::flyout()}
        ]
    }
}

fn top_bar() -> Part {
    let mut left = vec![
        icon_button(icons::logo(), 28.0),
        hgap(4.0),
        vline(16.0, tokens::BORDER),
        hgap(4.0),
    ];
    for item in ["Scene", "Project", "Debug", "Editor", "Help"] {
        if item == "Project" {
            left.push(Box::new(bsn! {
                Node {
                    height: px(27),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                }
                Button
                Children [ {menu_item(item)} ]
                on(files::open_project_menu)
            }) as Part);
        } else {
            let kind = match item {
                "Scene" => menu::TopKind::Scene,
                "Debug" => menu::TopKind::Debug,
                "Editor" => menu::TopKind::Editor,
                _ => menu::TopKind::Help,
            };
            left.push(Box::new(bsn! {
                Node {
                    height: px(27),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                }
                Button
                menu::TopMenuButton(kind)
                Children [ {menu_item(item)} ]
                on(menu::open_top_menu)
            }) as Part);
        }
    }

    let right = vec![
        build_dialog::top_bar_button(),
        hgap(6.0),
        crate::play::button(),
        icon_button(icons::play_scene(), 24.0),
        icon_button(icons::dropdown(), 20.0),
        vline(16.0, tokens::BORDER),
        icon_button(icons::network(), 24.0),
        icon_button(icons::branch(), 24.0),
        icon_button(icons::gear(), 24.0),
        icon_button(icons::account(), 24.0),
    ];

    Box::new(bsn! {
        Node {
            height: px(MENU_BAR_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            border: UiRect { bottom: px(1) },
            column_gap: px(4),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                column_gap: px(2),
            }
            Children [ {left} ]
            --
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(2),
            }
            Children [ {right} ]
        ]
    })
}

fn import_status() -> Part {
    Box::new(bsn! {
        Text("")
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        crate::import::ImportStatus
    })
}

pub(crate) fn status_bar() -> Part {
    Box::new(bsn! {
        Node {
            height: px(STATUS_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect { top: px(1) },
            column_gap: px(10),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {text(format!("{} {EDITOR_VERSION}", crate::PRODUCT), FONT_SIZE, tokens::TEXT_DIM)}
            --
            {import_status()}
            --
            {spacer()}
            --
            {icon_button(icons::branch(), 16.0)}
            --
            {icon_button(icons::file(), 16.0)}
            --
            {icon_button(icons::account(), 16.0)}
        ]
    })
}

/// Installs the 3D viewport, the scene state, and the dock sync systems.
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        viewport::plugins(app);
        dock::plugins(app);
        let extensions = eme_game_runtime::load_for(&build::workspace_dir());
        app.insert_resource(extensions.clone());
        app.add_plugins(eme_script::ScriptPlugin);
        if extensions.enabled(eme_game_runtime::Extension::Animation) {
            app.add_plugins(eme_anim::AnimationRuntimePlugin);
        }
        if extensions.enabled(eme_game_runtime::Extension::Navigation) {
            app.add_plugins(eme_game_runtime::NavPlugin);
        }
        app.configure_sets(
            Update,
            eme_game_runtime::NavSet.after(eme_script::update_scripts),
        );
        app.insert_resource(eme_script::ScriptRoot(crate::asset_root()));
        app.init_resource::<eme_game_runtime::ControllerSettings>();
        crate::settings::install(app);
        crate::mixer::install(app);
        crate::packages::install(app);
        crate::shortcuts::install(app);
        crate::extensions::install(app);
        crate::profile::install(app);
        crate::animation::install(app);
        crate::console_cmd::install(app);
        crate::layouts::install(app);
        crate::hud::install(app);
        app.init_resource::<scene::SceneState>()
            .init_resource::<scene::SceneAssets>()
            .init_resource::<undo::Undo>()
            .init_resource::<clipboard::Clipboard>()
            .init_resource::<viewport::ViewportRes>()
            .init_resource::<tabs::Tabs>()
            .init_resource::<crate::play::PlayState>()
            .init_resource::<crate::play::PlayHover>()
            .init_resource::<orbit::OrbitCamera>()
            .init_resource::<orbit::FlyCam>()
            .init_resource::<orbit::CameraMode>()
            .init_resource::<orbit::FlyMove>()
            .init_resource::<orbit::ViewMode>()
            .init_resource::<menu::MenuState>()
            .init_resource::<menu::TopMenu>()
            .init_resource::<menu::RightPress>()
            .init_resource::<build::BuildState>()
            .init_resource::<files::FileTree>()
            .init_resource::<crate::console::ConsoleLog>()
            .init_resource::<crate::console::ConsoleFilter>()
            .init_resource::<crate::import::ImportService>()
            .init_resource::<crate::import::ModelWatcher>()
            .init_resource::<scene::ModelIndex>()
            .init_resource::<scene::NodeSelection>()
            .init_resource::<crate::script_watch::ScriptWatcher>()
            .init_resource::<inspector::ReattachState>()
            .init_resource::<inspector::ScriptPicker>()
            .init_resource::<inspector::InspectorFilter>()
            .init_resource::<palette::Palette>()
            .init_resource::<docks::EditorStats>()
            .init_resource::<docks::SceneFilter>()
            .init_resource::<files::ImportMenu>()
            .init_resource::<files::FileFilter>()
            .init_resource::<files::AssetDrag>()
            .init_resource::<crate::import::OptionsPanel>()
                        .add_observer(menu::request_object_menu)
            .add_systems(
                Update,
                (
                    viewport::setup_viewport,
                    viewport::teardown_viewport,
                    viewport::sync_game_world,
                    (viewport::sync_editor_ui, viewport::sync_look_hint),
                    tabs::rebuild_strip,
                    scene::tint_selection,
                    scene::face_viewport_camera,
                    scene::keyboard_edit,
                    scene::draw_camera_frusta,
                    scene::sync_node_selection,
                    scene::report_override_issues,
                    crate::selection_outline::sync_outlines,
                    (
                        crate::console::sync,
                        crate::console::sync_filter,
                        crate::console::sync_filter_hint,
                        sync_script_activity,
                        drain_script_logs,
                        crate::import::poll_system,
                        crate::import::apply_reloads,
                        crate::import::status_system,
                        crate::import::sync_panel,
                        (
                            files::instance_imports,
                            files::drop_asset,
                            files::accept_drop,
                            files::sync_import_menu,
                            files::close_import_menu,
                        ),
                        crate::play::poll,
                        scene_transitions_in_player,
                        crate::play::sync_button,
                        crate::play::autoplay,
                        eme_game_runtime::run_behaviours.run_if(crate::play::behaviours_active),
                        eme_game_runtime::step_controllers
                            .after(eme_script::update_scripts)
                            .run_if(crate::play::behaviours_active)
                            .run_if(extension_physics),
                        eme_game_runtime::step_physics
                            .after(eme_script::update_scripts)
                            .run_if(crate::play::behaviours_active)
                            .run_if(extension_physics),
                        (crate::play::sync_flyout, crate::play::place_flyout),
                    ),
                    orbit::toggle_mode,
                    orbit::fly_move.before(orbit::apply_camera),
                    orbit::apply_camera,
                    orbit::reset_orbit,
                    docks::rebuild_scene_rows.in_set(crate::profile::UiProfileSet),
                    (
                        files::sync_file_rows,
                        files::sync_file_filter,
                        files::sync_file_filter_hint,
                        files::poll_files.run_if(eme::time::common_conditions::on_timer(
                            core::time::Duration::from_secs(3),
                        )),
                        crate::import::watch_models.run_if(eme::time::common_conditions::on_timer(
                            core::time::Duration::from_millis(500),
                        )),
                        crate::script_watch::watch_scripts.run_if(eme::time::common_conditions::on_timer(
                            core::time::Duration::from_millis(500),
                        )),
                    ),
                    (
                        inspector::sync_inspector,
                        inspector::sync_color_swatch,
                        inspector::sync_animation_rows,
                        inspector::sync_inspector_filter,
                        inspector::sync_inspector_filter_hint,
                        inspector::apply_inspector_filter,
                        inspector::sync_script_rows,
                        inspector::sync_script_picker,
                        inspector::sync_node_inspector,
                        inspector::sync_override_panel,
                        inspector::sync_import_report,
                        inspector::typing_guard.before(scene::keyboard_edit),
                    ),
                ),
            )
            .add_systems(
                Update,
                (
                    docks::update_stats,
                    docks::sample_gpu,
                    crate::memory::sample_memory,
                    docks::sync_stats,
                    docks::sync_outline,
                    docks::sync_scene_filter,
                    docks::sync_filter_hint,
                ),
            )
            .add_systems(
                Update,
                (
                    scene::respawn_model_roots,
                    (scene::sync_model_overrides, eme_game_overrides::sync_model_nodes).chain(),
                ),
            )
            .add_systems(
                Update,
                (
                    menu::sync_top_menu,
                    menu::close_top_menu,
                    (
                        crate::profile::begin_scripts.before(eme_script::update_scripts),
                        crate::profile::end_scripts.after(eme_script::update_scripts),
                        crate::profile::begin_physics.before(eme_game_runtime::step_physics),
                        crate::profile::end_physics.after(eme_game_runtime::step_physics),
                        crate::profile::begin_nav.before(eme_game_runtime::NavSet),
                        crate::profile::end_nav.after(eme_game_runtime::NavSet),
                        crate::profile::begin_ui.before(crate::profile::UiProfileSet),
                        crate::profile::end_ui.after(crate::profile::UiProfileSet),
                    ),
                    (palette::keys, palette::sync).chain(),
                    menu::seed_rename,
                    menu::rename_keys,
                    menu::sync_menu,
                    menu::style_menu_rows,
                    menu::close_on_teardown,
                    dock::sync.in_set(crate::profile::UiProfileSet),
                    persist::edit_shortcuts,
                    build::poll_build,
                    build_dialog::sync_modal,
                    build_dialog::style_buttons,
                    build_dialog::escape,
                    build_dialog::close_on_teardown,
                ),
            )
            .add_systems(Startup, autoload_scene)
            .add_systems(Startup, crate::selection_outline::init_outline_material)
            .add_systems(PostUpdate, scene::reconcile_scene)
            .add_systems(Last, scene::purge_stale);
    }
}

fn scene_transitions_in_player(
    mut requests: ResMut<eme_script::SceneRequests>,
    mut log: ResMut<crate::console::ConsoleLog>,
) {
    if let Some(path) = requests.load.take() {
        log.push(format!(
            "scene: load {path} ignored in the editor; run the player"
        ));
    }
    if requests.reload {
        requests.reload = false;
        log.push("scene: reload ignored in the editor; run the player".to_string());
    }
    for (id, path) in requests.additive.drain(..) {
        log.push(format!(
            "scene: additive load {path} (layer {id}) ignored in the editor; run the player"
        ));
    }
    for id in requests.unload.drain(..) {
        log.push(format!(
            "scene: unload of layer {id} ignored in the editor; run the player"
        ));
    }
}

fn extension_physics(ext: Res<eme_game_runtime::ExtensionSet>) -> bool {
    ext.enabled(eme_game_runtime::Extension::Physics)
}

fn sync_script_activity(
    play: Res<crate::play::PlayState>,
    view: Res<orbit::ViewMode>,
    ext: Res<eme_game_runtime::ExtensionSet>,
    mut active: ResMut<eme_script::ScriptsActive>,
) {
    let next = ext.enabled(eme_game_runtime::Extension::Scripting)
        && play.running()
        && *view == orbit::ViewMode::Game;
    if active.0 != next {
        active.0 = next;
    }
}

fn drain_script_logs(
    mut scripts: ResMut<eme_script::ScriptLog>,
    mut console: ResMut<crate::console::ConsoleLog>,
    mut seen: Local<u64>,
) {
    if scripts.rev() == *seen {
        return;
    }
    *seen = scripts.rev();
    for line in scripts.drain() {
        console.push(line);
    }
}

fn autoload_scene(
    mut state: ResMut<scene::SceneState>,
    mut node: ResMut<scene::NodeSelection>,
    index: Res<scene::ModelIndex>,
    mut log: ResMut<crate::console::ConsoleLog>,
) {
    if std::env::var_os("EME_EDITOR_DEMO").is_some() {
        return;
    }
    // headless captures normally start empty; an explicit scene is loaded even then
    let explicit = std::env::var_os("EME_EDITOR_SCENE").map(std::path::PathBuf::from);
    if explicit.is_none() && std::env::var_os("EME_EDITOR_SHOT").is_some() {
        return;
    }
    let loaded = match &explicit {
        Some(path) => persist::load_from(&path.to_string_lossy()),
        None => persist::load(),
    };
    if let Ok(Some(mut doc)) = loaded {
        crate::import::migrate_doc(&mut doc, &crate::asset_root(), &index.0, &mut log);
        let count = doc.len();
        state.replace_doc(doc);
        info!("loaded {count} objects");
        if let Some(id) = std::env::var("EME_EDITOR_SELECT")
            .ok()
            .and_then(|id| id.parse::<u64>().ok())
        {
            state.select_only(id);
        }
        if let Some(spec) = std::env::var("EME_EDITOR_NODE").ok()
            && let Some((path, name)) = spec.split_once(':')
            && let Some(object) = state.primary()
        {
            let path = path
                .split('.')
                .filter_map(|part| part.parse::<u32>().ok())
                .collect();
            node.object = Some(object);
            node.key = Some(eme_game_scene::NodeKey {
                path,
                name: name.to_owned(),
            });
        }
    }
}
