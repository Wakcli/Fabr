//! The editor frame: a docked layout, monochrome chrome, and a perspective viewport.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use eme::prelude::*;
use eme::scene::{bsn, Scene};
use eme_editor_core::{Layout, EDITOR_VERSION};

use crate::tokens;

mod build_dialog;
pub(crate) mod chrome;
mod dock;
mod docks;
pub(crate) mod files;
mod gizmo;
pub(crate) mod icons;
mod inspector;
mod menu;
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
        left.push(menu_item(item));
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
            .init_resource::<menu::RightPress>()
            .init_resource::<build::BuildState>()
            .init_resource::<files::FileTree>()
            .init_resource::<crate::console::ConsoleLog>()
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
                    crate::selection_outline::sync_outlines,
                    (
                        crate::console::sync,
                    crate::play::poll,
                        crate::play::sync_button,
                        crate::play::autoplay,
                        eme_game_runtime::run_behaviours.run_if(crate::play::behaviours_active),
                    ),
                    (crate::play::sync_flyout, crate::play::place_flyout),
                    orbit::toggle_mode,
                    orbit::fly_move.before(orbit::apply_camera),
                    orbit::apply_camera,
                    orbit::reset_orbit,
                    docks::rebuild_scene_rows,
                    (
                        files::sync_file_rows,
                        files::poll_files.run_if(eme::time::common_conditions::on_timer(
                            core::time::Duration::from_secs(3),
                        )),
                    ),
                    inspector::sync_inspector,
                    inspector::typing_guard.before(scene::keyboard_edit),
                ),
            )
            .add_systems(
                Update,
                (
                    menu::seed_rename,
                    menu::rename_keys,
                    menu::sync_menu,
                    menu::style_menu_rows,
                    menu::close_on_teardown,
                    dock::sync,
                    persist::scene_shortcuts,
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

fn autoload_scene(mut state: ResMut<scene::SceneState>) {
    if std::env::var_os("EME_EDITOR_DEMO").is_some()
        || std::env::var_os("EME_EDITOR_SHOT").is_some()
    {
        return;
    }
    if let Ok(Some(doc)) = persist::load() {
        let count = doc.len();
        state.replace_doc(doc);
        info!("loaded {count} objects from {}", persist::SCENE_PATH);
    }
}
