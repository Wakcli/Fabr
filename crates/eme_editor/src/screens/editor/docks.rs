//! The Scene, `FileSystem`, Output, and History docks.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button};
use eme_game_scene::ObjectKind;

use super::chrome::*;
use super::icons;
use super::menu::{self, MenuState};
use crate::scene::SceneState;
use crate::tokens;

pub fn left_dock() -> Part {
    let scene = scene_dock();
    let files = filesystem_dock();

    Box::new(bsn! {
        Node {
            width: px(LEFT_DOCK_W),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            Node {
                flex_grow: 0.62,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {scene} ]
            --
            {splitter(false)}
            --
            Node {
                flex_grow: 0.38,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {files} ]
        ]
    })
}

fn dock_frame(tabs: Vec<Part>, tail: Vec<Part>, body: Part, strip: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            {tab_strip(tabs, tail, strip)}
            --
            {body}
        ]
    })
}

fn dock_menu() -> Part {
    icon_button(icons::dock_menu(), 20.0)
}

pub fn scene_dock() -> Part {
    let tabs = vec![tab("Scene", true), tab("Import", false)];
    let tail = vec![dock_menu()];

    let toolbar = Box::new(bsn! {
        Node {
            height: px(TOOL_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(4)),
            column_gap: px(1),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {menu::add_button()}
            --
            {icon_button(icons::instance(), 24.0)}
            --
            {icon_button(icons::attach_script(), 24.0)}
            --
            {spacer()}
            --
            {icon_button(icons::search(), 24.0)}
        ]
    });

    let filter = Box::new(bsn! {
        Node {
            padding: UiRect {
                left: px(6),
                right: px(6),
                top: px(4),
                bottom: px(5),
            },
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            {field(
                vec![
                    icons::search_dim(),
                    text("Filter nodes", FONT_SIZE, tokens::TEXT_DIM),
                ],
                FIELD_H,
            )}
        ]
    });

    let tree = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { top: px(3) },
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::INSET)
        on(open_tree_menu)
        Children [ {tree_rows()} ]
    });

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            {toolbar}
            --
            {filter}
            --
            {tree}
        ]
    });

    dock_frame(tabs, tail, body, tokens::WINDOW)
}

fn tree_rows() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        SceneRows
    })
}

/// Host for the rows that mirror the viewport scene.
#[derive(Component, Clone, Default)]
pub struct SceneRows;

/// Scene-tree row pointing at one spawned object.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneRow(pub u64);

/// The icon a scene kind shows in the tree.
fn kind_badge(kind: ObjectKind) -> Part {
    match kind {
        ObjectKind::Empty => icons::mono(icons::EMPTY, tokens::CLASS_NODE),
        ObjectKind::Cube => icons::badge(icons::MESH),
        ObjectKind::Sphere => icons::mono(icons::SPHERE, tokens::CLASS_MESH),
        ObjectKind::Cylinder => icons::mono(icons::CYLINDER, tokens::CLASS_MESH),
        ObjectKind::Plane => icons::mono(icons::PLANE, tokens::CLASS_MESH),
        ObjectKind::PointLight | ObjectKind::Sun => icons::badge(icons::LIGHT),
        ObjectKind::Camera => icons::badge(icons::CAMERA),
    }
}

/// Rebuilds the scene rows whenever objects or selection change.
pub fn rebuild_scene_rows(
    mut commands: Commands,
    hosts: Query<Entity, With<SceneRows>>,
    state: Res<SceneState>,
    mut last: Local<Option<(Entity, u64)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    if *last == Some((host, state.rev)) {
        return;
    }
    *last = Some((host, state.rev));
    commands.entity(host).despawn_children();

    if state.doc.is_empty() {
        let empty = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(host)
            Children [ {text("No objects in scene", FONT_SIZE, tokens::TEXT_DIM)} ]
        };
        commands.spawn_scene(empty);
        return;
    }

    for obj in &state.doc.objects {
        let selected = state.selected == Some(obj.id);
        let id = obj.id;
        let row = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(8),
                    right: px(6),
                },
                column_gap: px(6),
            }
            ThemeBackgroundColor({if selected { tokens::SELECT } else { tokens::CLEAR }})
            Button
            SceneRow(id)
            ChildOf(host)
            Children [
                {kind_badge(obj.kind)}
                --
                {text(
                    obj.name.clone(),
                    FONT_SIZE,
                    if selected { tokens::SELECT_TEXT } else { tokens::TEXT },
                )}
            ]
            on(select_row)
            on(open_row_menu)
        };
        commands.spawn_scene(row);
    }
}

fn select_row(activate: On<Activate>, rows: Query<&SceneRow>, mut state: ResMut<SceneState>) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.select(Some(row.0));
}

/// Right click selects the row and opens the object menu for it.
fn open_row_menu(
    mut click: On<PointerClick>,
    rows: Query<&SceneRow>,
    mut menu: ResMut<MenuState>,
    mut state: ResMut<SceneState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    click.propagate(false);
    state.select(Some(row.0));
    menu::open(&mut menu, click.pointer.position, Some(row.0));
}

/// Right click on the tree background opens the menu for the selection.
fn open_tree_menu(
    mut click: On<PointerClick>,
    mut menu: ResMut<MenuState>,
    state: Res<SceneState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    click.propagate(false);
    menu::open(&mut menu, click.pointer.position, state.selected);
}

pub fn filesystem_dock() -> Part {
    let tabs = vec![tab("FileSystem", true)];
    let tail = vec![dock_menu()];

    let mut folders = Vec::new();
    for name in ["scenes", "scripts", "assets", "models"] {
        folders.push(folder_cell(name));
    }

    let mut files = Vec::new();
    for name in [
        "project.ember",
        "main.scene",
        "player.scene",
        "world.scene",
        "player.rs",
        "world.material",
        "icon.svg",
    ] {
        files.push(file_row(name));
    }

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { bottom: px(4) },
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::INSET)
        Children [
            Node {
                height: px(24),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(7)),
                column_gap: px(3),
            }
            Children [
                {icon_button(icons::folder(), 16.0)}
                --
                {text("project://", FONT_SIZE, tokens::TEXT_MEDIUM)}
                --
                {icon_button(icons::breadcrumb_caret(), 12.0)}
            ]
            --
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                padding: UiRect {
                    left: px(4),
                    right: px(4),
                },
                column_gap: px(2),
            }
            Children [ {folders} ]
            --
            {files}
        ]
    });

    dock_frame(tabs, tail, body, tokens::WINDOW)
}

fn folder_cell(name: &str) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(66),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: px(4),
        }
        Children [
            {icons::folder_large()}
            --
            {text(name, FONT_SIZE, tokens::TEXT_MEDIUM)}
        ]
    })
}

fn file_row(name: &str) -> Part {
    Box::new(bsn! {
        Node {
            height: px(TREE_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(6),
        }
        Children [
            {icons::file()}
            --
            {text(name, FONT_SIZE, tokens::TEXT)}
        ]
    })
}

pub fn bottom_dock() -> Part {
    let tabs = vec![
        bottom_tab("Output", false),
        bottom_tab("Debugger", true),
        bottom_tab("Audio", false),
        bottom_tab("Animation", false),
    ];
    let tail = vec![
        icon_button(icons::close(), 18.0),
        icon_button(icons::dock_menu(), 20.0),
    ];

    let mut lines = Vec::new();
    for line in [
        "Emberrly Engine v0.20.0-dev (c) 2026 Emberrly Contributors.",
        "Vulkan 1.3.280 - Clustered Forward - Device #0",
        "Loaded project: project://project.ember",
    ] {
        lines.push(text_mono(line, FONT_SIZE_OUTPUT, tokens::TEXT));
    }

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect {
                left: px(8),
                right: px(8),
                top: px(6),
            },
            row_gap: px(2),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::INSET)
        Children [ {lines} ]
    });

    dock_frame(tabs, tail, body, tokens::PANEL)
}

pub fn history_dock() -> Part {
    let tabs = vec![tab("Node", false), tab("History", true)];
    let tail = vec![
        icon_button(icons::close(), 18.0),
        icon_button(icons::dock_menu(), 20.0),
    ];

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [ {text("No history", FONT_SIZE, tokens::TEXT_DIM)} ]
    });

    dock_frame(tabs, tail, body, tokens::WINDOW)
}
