//! Center column: scene tabs, the perspective viewport, and the bottom dock.

use eme::camera::RenderTarget;
use eme::dev_tools::infinite_grid::{InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings};
use eme::feathers::{
    constants::fonts,
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken, UiTheme},
};
use eme::gizmos::transform_gizmo::{TransformGizmoCamera, TransformGizmoMode};
use eme::math::Rot2;
use eme::prelude::*;
use eme::render::render_resource::TextureFormat;
use eme::scene::bsn;
use eme::text::{FontSize, FontSourceTemplate, FontWeight, TextFont};
use eme::ui::widget::ViewportNode;
use eme::ui::UiTransform;
use eme::window::{CursorOptions, PrimaryWindow, Window};

use super::chrome::*;
use super::gizmo::{tool_button, toolbar_button, GizmoPlugin, ToolModeText, ToolbarToggleKind};
use super::{docks, icons, menu};
use crate::orbit::{orbit_drag, orbit_scroll};
use crate::scene;
use crate::tokens;

/// Camera position for the editor's perspective view.
const CAMERA: Vec3 = Vec3::new(7.0, 5.0, 7.0);

/// Size of the right-drag look glyph, in logical pixels.
const LOOK_HINT_SIZE: f32 = 32.0;

/// Marks the viewport panel so the 3D camera attaches once the node exists.
#[derive(Component, Clone, Copy, Default)]
pub struct ViewportPanel;

/// The eye glyph that follows the pointer while right-dragging to look around.
#[derive(Component, Clone, Copy, Default)]
pub struct LookHint;

/// UI that is only shown while editing, not in the Game view.
#[derive(Component, Clone, Copy, Default)]
pub struct EditorOnly;

/// Render entities owned by the viewport, despawned with the editor screen.
#[derive(Resource, Default)]
pub struct ViewportRes {
    pub camera: Option<Entity>,
    props: Vec<Entity>,
}

pub fn center() -> Part {
    let tabs = scene_tabs();
    let view = viewport();
    let bottom = docks::bottom_dock();

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            {tabs}
            --
            {view}
            --
            {splitter(false)}
            --
            Node {
                height: px(BOTTOM_DOCK_H),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {bottom} ]
        ]
    })
}

fn scene_tabs() -> Part {
    let kids = vec![
        scene_tab("main.scene"),
        spacer(),
        icon_button(icons::add(), 24.0),
        icon_button(icons::dock_menu(), 20.0),
    ];

    Box::new(bsn! {
        Node {
            height: px(SCENE_TAB_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            padding: UiRect { right: px(6) },
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {kids} ]
    })
}

fn scene_tab(label: &str) -> Part {
    Box::new(bsn! {
        Node {
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(11)),
            border: UiRect { bottom: px(2) },
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::TAB_HIGHLIGHT)
        Children [
            {icons::badge(icons::SCENE)}
            --
            {text(label, FONT_SIZE, tokens::TEXT)}
        ]
    })
}

fn viewport() -> Part {
    let tools = transform_toolbar();
    let perspective = perspective_control();
    let gizmo = axis_gizmo();

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            position_type: PositionType::Relative,
            overflow: Overflow::clip(),
        }
        ViewportPanel
        on(orbit_drag)
        on(orbit_scroll)
        on(menu::arm_viewport_menu)
        on(menu::viewport_menu)
        ThemeBackgroundColor(tokens::VIEWPORT_BOTTOM)
        Children [
            {tools}
            --
            {perspective}
            --
            {gizmo}
            --
            {look_hint()}
        ]
    })
}

fn look_hint() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: px(LOOK_HINT_SIZE),
            height: px(LOOK_HINT_SIZE),
            display: Display::None,
        }
        LookHint
        GlobalZIndex(20)
        Pickable::IGNORE
        Children [ {icons::look(LOOK_HINT_SIZE)} ]
    })
}

/// Follows the pointer with the look glyph while the right button is held.
pub fn sync_look_hint(
    mouse: Res<ButtonInput<MouseButton>>,
    view: Res<crate::orbit::ViewMode>,
    window: Single<&Window, With<PrimaryWindow>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<ViewportPanel>>,
    mut cursor: Single<&mut CursorOptions>,
    mut hints: Query<&mut Node, With<LookHint>>,
) {
    let Ok((panel, global)) = panels.single() else {
        return;
    };
    let rect = Rect::from_center_size(global.translation.trunc(), panel.size());
    let pointer = window.cursor_position().map(|pos| pos * window.scale_factor());
    let inside = pointer.is_some_and(|pos| rect.contains(pos));
    let show =
        mouse.pressed(MouseButton::Right) && inside && *view == crate::orbit::ViewMode::Editor;
    let hide_cursor = show && window.focused;

    if cursor.visible == hide_cursor {
        cursor.visible = !hide_cursor;
    }

    for mut node in &mut hints {
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        if let (true, Some(pos)) = (show, pointer) {
            let local = pos - rect.min;
            node.left = px(local.x - LOOK_HINT_SIZE / 2.0);
            node.top = px(local.y - LOOK_HINT_SIZE / 2.0);
        }
    }
}

fn overlay(kids: Vec<Part>) -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: px(2),
            column_gap: px(1),
            border: px(1),
            border_radius: px(4),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {kids} ]
    })
}

fn transform_toolbar() -> Part {
    let kids = vec![
        toolbar_button(
            icons::mono(icons::TOOL_SELECT, tokens::ICON),
            ToolbarToggleKind::Select,
        ),
        vline(14.0, tokens::BORDER),
        tool_button(
            icons::mono(icons::TOOL_MOVE, tokens::ICON),
            TransformGizmoMode::Translate,
        ),
        tool_button(
            icons::mono(icons::TOOL_ROTATE, tokens::ICON),
            TransformGizmoMode::Rotate,
        ),
        tool_button(
            icons::mono(icons::TOOL_SCALE, tokens::ICON),
            TransformGizmoMode::Scale,
        ),
        vline(14.0, tokens::BORDER),
        toolbar_button(
            icons::mono(icons::SNAP, tokens::ICON),
            ToolbarToggleKind::Snap,
        ),
        toolbar_button(
            icons::mono(icons::TOOL_TRANSFORM, tokens::ICON),
            ToolbarToggleKind::Space,
        ),
        vline(14.0, tokens::BORDER),
        mode_readout(),
    ];

    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(8),
            top: px(8),
        }
        EditorOnly
        Children [ {overlay(kids)} ]
    })
}

fn mode_readout() -> Part {
    Box::new(bsn! {
        Node {
            height: px(26),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(5)),
        }
        Children [
            Text("Move")
            TextFont {
                font: FontSourceTemplate::Handle(fonts::REGULAR),
                font_size: FontSize::Px(12.0),
                weight: FontWeight::NORMAL,
            }
            ThemeTextColor(tokens::TEXT_MEDIUM)
            ToolModeText
        ]
    })
}

fn perspective_control() -> Part {
    let dropdown = Box::new(bsn! {
        Node {
            height: px(24),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(7),
            border: px(1),
            border_radius: px(4),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {text("Perspective", FONT_SIZE, tokens::TEXT)}
            --
            {icons::mono(icons::CHEVRON_DOWN, tokens::TEXT_MEDIUM)}
        ]
    });

    let zoom = Box::new(bsn! {
        Node {
            width: px(24),
            height: px(24),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: px(1),
            border_radius: px(4),
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {icons::mono(icons::ADD, tokens::TEXT_MEDIUM)} ]
    });

    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            right: px(8),
            top: px(8),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(4),
        }
        EditorOnly
        Children [ {dropdown} -- {zoom} ]
    })
}

fn label_at(value: &str, x: f32, y: f32, token: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(x),
            top: px(y),
        }
        Children [ {text_bold(value, 11.0, token)} ]
    })
}

fn axis_gizmo() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            bottom: px(12),
            width: px(52),
            height: px(52),
        }
        EditorOnly
        Children [
            {bar(1.6, 16.0, 25.2, 14.0, 0.0, tokens::AXIS_Y)}
            --
            {bar(1.6, 16.0, 31.8, 26.5, -56.0, tokens::AXIS_X)}
            --
            {bar(1.6, 16.0, 18.6, 26.5, 56.0, tokens::AXIS_Z)}
            --
            {label_at("Y", 22.2, 2.0, tokens::AXIS_Y)}
            --
            {label_at("X", 41.5, 40.0, tokens::AXIS_X)}
            --
            {label_at("Z", 3.0, 40.0, tokens::AXIS_Z)}
            --
            {dot(3.0, 24.5, 28.5, tokens::TEXT_MEDIUM)}
        ]
    })
}

fn bar(width: f32, height: f32, left: f32, top: f32, angle: f32, token: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: px(width),
            height: px(height),
            left: px(left),
            top: px(top),
            border_radius: px(width / 2.0),
        }
        ThemeBackgroundColor(token)
        UiTransform::from_rotation(Rot2::degrees(angle))
    })
}

fn dot(size: f32, left: f32, top: f32, token: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: px(size),
            height: px(size),
            left: px(left),
            top: px(top),
            border_radius: px(size / 2.0),
        }
        ThemeBackgroundColor(token)
    })
}

/// Hides the grid and camera glyph while the Game view is active.
pub fn sync_game_world(
    view: Res<crate::orbit::ViewMode>,
    mut glyphs: Query<&mut Visibility, (With<scene::CameraGlyph>, Without<InfiniteGrid>)>,
    mut grids: Query<&mut Visibility, (With<InfiniteGrid>, Without<scene::CameraGlyph>)>,
) {
    let want = if *view == crate::orbit::ViewMode::Game {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut vis in &mut glyphs {
        if *vis != want {
            *vis = want;
        }
    }
    for mut vis in &mut grids {
        if *vis != want {
            *vis = want;
        }
    }
}

/// Shows the editor-only overlays outside Game view.
pub fn sync_editor_ui(
    view: Res<crate::orbit::ViewMode>,
    mut nodes: Query<&mut Node, With<EditorOnly>>,
) {
    let want = if *view == crate::orbit::ViewMode::Game {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut nodes {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Creates the 3D camera and the static scene once the viewport node exists.
pub fn setup_viewport(
    mut commands: Commands,
    panels: Query<Entity, (With<ViewportPanel>, Without<ViewportNode>)>,
    mut view: ResMut<ViewportRes>,
    mut images: ResMut<Assets<Image>>,
    theme: Res<UiTheme>,
    mut state: ResMut<scene::SceneState>,
) {
    let Ok(panel) = panels.single() else {
        return;
    };

    let image = Image::new_target_texture(0, 0, TextureFormat::Bgra8UnormSrgb, None);
    let handle = images.add(image);
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                ..default()
            },
            RenderTarget::Image(handle.into()),
            Transform::from_translation(CAMERA).looking_at(Vec3::ZERO, Vec3::Y),
            scene::ViewportCamera,
            MeshPickingCamera,
            TransformGizmoCamera,
        ))
        .id();
    commands.entity(panel).insert(ViewportNode::new(camera));

    let sun = commands
        .spawn((
            DirectionalLight {
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();

    let grid = commands
        .spawn((
            InfiniteGrid,
            InfiniteGridSettings {
                x_axis_color: theme.color(&tokens::AXIS_X),
                z_axis_color: theme.color(&tokens::AXIS_Z),
                minor_line_color: theme.color(&tokens::SEPARATOR),
                major_line_color: theme.color(&tokens::BORDER),
                ..default()
            },
        ))
        .id();

    view.camera = Some(camera);
    view.props = vec![sun, grid];

    scene::spawn_demo(&mut state);
}

/// Tears the 3D scene down with the editor screen so the hub stays untouched.
pub fn teardown_viewport(
    mut commands: Commands,
    panels: Query<(), With<ViewportPanel>>,
    mut view: ResMut<ViewportRes>,
    mut state: ResMut<scene::SceneState>,
) {
    if !panels.is_empty() {
        return;
    }
    let Some(camera) = view.camera.take() else {
        return;
    };
    commands.entity(camera).despawn();
    for entity in view.props.drain(..) {
        commands.entity(entity).despawn();
    }
    scene::clear_scene(&mut state);
}

/// Installs the viewport's 3D rendering plugins.
pub fn plugins(app: &mut App) {
    app.add_plugins((MeshPickingPlugin, InfiniteGridPlugin, GizmoPlugin));
}
