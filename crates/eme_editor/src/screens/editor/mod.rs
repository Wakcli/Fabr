//! The editor frame: a docked layout, monochrome chrome, and a perspective viewport.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, UiTheme};
use eme::prelude::*;
use eme::scene::{bsn, Scene};
use eme::text::TextColor;
use eme::text::{FontSize, FontSourceTemplate, FontWeight, TextFont};
use eme::ui::widget::Text;
use eme::ui::BackgroundColor;
use eme::ui_widgets::Button;
use eme_editor_core::{Layout, EDITOR_VERSION};

use crate::tokens;

mod build_dialog;
mod chrome;
mod docks;
mod gizmo;
mod icons;
mod inspector;
mod menu;
mod viewport;

use crate::build;
use crate::orbit;
use crate::persist;
use crate::scene;
use chrome::*;

/// Editor frame scene.
pub fn screen(_layout: Layout) -> impl Scene {
    let bar = top_bar();
    let body = body();
    let status = status_bar();

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
            {body}
            --
            {status}
        ]
    }
}

fn body() -> Part {
    let left = docks::left_dock();
    let center = viewport::center();
    let right = inspector::right_dock();

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
        }
        Children [
            {left}
            --
            {splitter(true)}
            --
            {center}
            --
            {splitter(true)}
            --
            {right}
        ]
    })
}

/// A main-screen tab that switches the viewport between Editor and Game.
#[derive(Component, Clone, Copy, Default)]
struct ViewTab(orbit::ViewMode);

#[derive(Component, Clone, Copy, Default)]
struct ViewTabLabel(orbit::ViewMode);

fn view_tab(label: &str, mode: orbit::ViewMode) -> Part {
    Box::new(bsn! {
        Node {
            height: percent(100),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(9)),
            border_radius: px(3),
        }
        Button
        ViewTab(mode)
        BackgroundColor(Color::NONE)
        Children [ {view_tab_label(label, mode)} ]
        on(view_tab_click)
    })
}

fn view_tab_label(label: &str, mode: orbit::ViewMode) -> Part {
    let value = label.to_string();
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::BOLD),
            font_size: FontSize::Px(FONT_SIZE_MAIN_SCREEN),
            weight: FontWeight::NORMAL,
        }
        TextColor(Color::WHITE)
        ViewTabLabel(mode)
    })
}

fn view_tab_click(
    click: On<PointerClick>,
    tabs: Query<&ViewTab>,
    parents: Query<&ChildOf>,
    mut mode: ResMut<orbit::ViewMode>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.entity;
    loop {
        if let Ok(tab) = tabs.get(entity) {
            if *mode != tab.0 {
                *mode = tab.0;
            }
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

fn sync_view_tabs(
    theme: Res<UiTheme>,
    mode: Res<orbit::ViewMode>,
    mut buttons: Query<(&ViewTab, &mut BackgroundColor)>,
    mut labels: Query<(&ViewTabLabel, &mut TextColor)>,
) {
    let panel = theme.color(&tokens::PANEL);
    let accent = theme.color(&tokens::ACCENT);
    let dim = theme.color(&tokens::TEXT_MEDIUM);
    for (tab, mut bg) in &mut buttons {
        let want = if tab.0 == *mode { panel } else { Color::NONE };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (tab, mut fg) in &mut labels {
        let want = if tab.0 == *mode { accent } else { dim };
        if fg.0 != want {
            fg.0 = want;
        }
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

    let mut center = Vec::new();
    for (label, mode) in [
        ("2D", None),
        ("3D", Some(orbit::ViewMode::Editor)),
        ("Script", None),
        ("Game", Some(orbit::ViewMode::Game)),
        ("AssetLib", None),
    ] {
        match mode {
            Some(mode) => center.push(view_tab(label, mode)),
            None => center.push(main_screen_button(label, false)),
        }
    }

    let right = vec![
        build_dialog::top_bar_button(),
        hgap(6.0),
        icon_button(icons::play(), 24.0),
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
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(2),
            }
            Children [ {center} ]
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

fn status_bar() -> Part {
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
            {text(format!("Emberrly {EDITOR_VERSION}"), FONT_SIZE, tokens::TEXT_DIM)}
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
        app.init_resource::<scene::SceneState>()
            .init_resource::<scene::SceneAssets>()
            .init_resource::<viewport::ViewportRes>()
            .init_resource::<orbit::OrbitCamera>()
            .init_resource::<orbit::FlyCam>()
            .init_resource::<orbit::CameraMode>()
            .init_resource::<orbit::FlyMove>()
            .init_resource::<orbit::ViewMode>()
            .init_resource::<menu::MenuState>()
            .init_resource::<menu::RightPress>()
            .init_resource::<build::BuildState>()
            .add_observer(menu::request_object_menu)
            .add_systems(
                Update,
                (
                    viewport::setup_viewport,
                    viewport::teardown_viewport,
                    viewport::sync_game_world,
                    viewport::sync_editor_ui,
                    sync_view_tabs,
                    scene::tint_selection,
                    scene::face_viewport_camera,
                    scene::keyboard_edit,
                    scene::draw_camera_frusta,
                    orbit::toggle_mode,
                    orbit::fly_move.before(orbit::apply_camera),
                    orbit::apply_camera,
                    orbit::reset_orbit,
                    docks::rebuild_scene_rows,
                    inspector::sync_inspector,
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
                    persist::scene_shortcuts,
                    build::poll_build,
                    build_dialog::sync_modal,
                    build_dialog::style_buttons,
                    build_dialog::escape,
                    build_dialog::close_on_teardown,
                ),
            )
            .add_systems(Startup, autoload_scene)
            .add_systems(PostUpdate, scene::reconcile_scene);
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
