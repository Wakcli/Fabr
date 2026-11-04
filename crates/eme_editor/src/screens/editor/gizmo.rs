//! Editor side of the engine's transform gizmo: focus follows the selection,
//! edits land back in the document, and the toolbar and hotkeys drive the mode.

use eme::feathers::theme::UiTheme;
use eme::gizmos::transform_gizmo::{
    TransformGizmoCursor, TransformGizmoFocus, TransformGizmoMode, TransformGizmoPlugin,
    TransformGizmoSettings, TransformGizmoState, TransformGizmoSystems,
};
use eme::input_focus::InputFocus;
use eme::math::Rect;
use eme::prelude::*;
use eme::text::EditableText;
use eme::ui::BackgroundColor;
use eme::ui_widgets::Button;
use eme::window::{PrimaryWindow, Window};

use super::viewport::ViewportPanel;

use super::chrome::Part;
use crate::orbit::ViewMode;
use crate::scene::{SceneLink, SceneState};
use crate::tokens;
use crate::undo::Undo;
use eme::math::ops;
use eme_game_scene::ObjectTransform;

/// Marks the toolbar button that switches to a gizmo mode.
#[derive(Component, Clone, Copy, Default)]
pub struct ToolButton(pub TransformGizmoMode);

/// Marks the label that names the active gizmo mode.
#[derive(Component, Clone, Copy, Default)]
pub struct ToolModeText;

fn mode_label(mode: TransformGizmoMode) -> &'static str {
    match mode {
        TransformGizmoMode::Translate => "Move",
        TransformGizmoMode::Rotate => "Rotate",
        TransformGizmoMode::Scale => "Scale",
    }
}

#[derive(Resource, Default)]
struct FocusTag(Option<Entity>);

/// Wires the transform gizmo into the viewport and keeps it in sync with the document.
pub struct GizmoPlugin;

impl Plugin for GizmoPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .init_resource::<FocusTag>()
            .add_systems(
                Update,
                (gizmo_hotkeys, sync_focus, sync_toolbar, sync_pointer).chain(),
            )
            .add_systems(
                PostUpdate,
                sync_doc
                    .after(TransformGizmoSystems)
                    .before(crate::scene::reconcile_scene),
            );
    }
}

/// `W` translates, `S` scales, `R` rotates.
fn gizmo_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<InputFocus>,
    texts: Query<(), With<EditableText>>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if mouse.pressed(MouseButton::Right) {
        return;
    }
    if let Some(entity) = focus.get()
        && texts.contains(entity)
    {
        return;
    }
    let mode = if keys.just_pressed(KeyCode::KeyW) {
        TransformGizmoMode::Translate
    } else if keys.just_pressed(KeyCode::KeyS) {
        TransformGizmoMode::Scale
    } else if keys.just_pressed(KeyCode::KeyR) {
        TransformGizmoMode::Rotate
    } else {
        return;
    };
    if settings.mode != mode {
        settings.mode = mode;
    }
}

/// Moves the gizmo focus onto the primary selection, and off the previous one.
///
/// The engine gizmo runs on a `Single` focus entity: a second one silently
/// disables hover and the gizmo mesh, so only the primary ever carries it.
fn sync_focus(
    mut commands: Commands,
    state: Res<SceneState>,
    view: Res<ViewMode>,
    mut tag: ResMut<FocusTag>,
) {
    let want = if *view == ViewMode::Game {
        None
    } else {
        state
            .primary()
            .and_then(|id| state.entities.get(&id).copied())
    };
    if tag.0 == want {
        return;
    }
    if let Some(old) = tag.0
        && let Ok(mut entity) = commands.get_entity(old)
    {
        entity.try_remove::<TransformGizmoFocus>();
    }
    if let Some(new) = want
        && let Ok(mut entity) = commands.get_entity(new)
    {
        entity.insert(TransformGizmoFocus);
    }
    tag.0 = want;
}

/// The transform `start` ends up at after the gizmo moved a body from `from` to `to`.
fn moved_transform(start: &ObjectTransform, from: &Transform, to: &Transform) -> ObjectTransform {
    let delta = to.translation - from.translation;
    let spin = to.rotation * from.rotation.inverse();
    let base = from.scale.to_array();
    let want = to.scale.to_array();
    let mut scale = start.scale;
    for (i, b) in base.iter().enumerate() {
        if ops::abs(*b) > f32::EPSILON {
            scale[i] = start.scale[i] * want[i] / b;
        }
    }
    ObjectTransform {
        translation: [
            start.translation[0] + delta.x,
            start.translation[1] + delta.y,
            start.translation[2] + delta.z,
        ],
        rotation: (spin * Quat::from_array(start.rotation)).to_array(),
        scale,
    }
}

/// Writes the gizmo's delta into every non-primary selected object's document.
fn apply_to_selection(
    state: &mut SceneState,
    from: &Transform,
    to: &Transform,
    others: &[(u64, ObjectTransform)],
) {
    for (id, start) in others {
        let want = moved_transform(start, from, to);
        let Some(obj) = state.doc.get_mut(*id) else {
            continue;
        };
        if obj.translation != want.translation {
            obj.translation = want.translation;
        }
        if obj.rotation != want.rotation {
            obj.rotation = want.rotation;
        }
        if obj.scale != want.scale {
            obj.scale = want.scale;
        }
    }
}

/// Mirrors the focused entity back into the document after the gizmo moved it,
/// and carries the same delta onto the rest of the selection.
///
/// The document stays the source of truth: the entity is only a projection of it,
/// so a drag has to be written back before the next reconcile overwrites the entity.
fn sync_doc(
    mut state: ResMut<SceneState>,
    gizmo: Res<TransformGizmoState>,
    focused: Query<(&SceneLink, &Transform), With<TransformGizmoFocus>>,
    mut undo: ResMut<Undo>,
    mut was_active: Local<bool>,
    mut others: Local<Vec<(u64, ObjectTransform)>>,
) {
    let was = *was_active;
    *was_active = gizmo.active;

    if gizmo.active && !was {
        undo.checkpoint(&state, "Transform");
        others.clear();
        let primary = state.primary();
        for id in state.selected_ids().filter(|id| Some(*id) != primary) {
            let Some(obj) = state.doc.get(id) else {
                continue;
            };
            others.push((id, obj.transform()));
        }
    }

    let Ok((link, tf)) = focused.single() else {
        if was && !gizmo.active {
            state.touch();
        }
        return;
    };
    let translation = tf.translation.to_array();
    let rotation = tf.rotation.to_array();
    let scale = tf.scale.to_array();

    let Some(obj) = state.doc.get_mut(link.id) else {
        return;
    };
    if obj.translation != translation {
        obj.translation = translation;
    }
    if obj.rotation != rotation {
        obj.rotation = rotation;
    }
    if obj.scale != scale {
        obj.scale = scale;
    }

    if gizmo.active {
        apply_to_selection(&mut state, &gizmo.start_transform, tf, &others);
    }

    if was && !gizmo.active {
        state.touch();
    }
}

/// Feeds the gizmo a pointer position in the viewport image's coordinate space.
fn sync_pointer(
    window: Single<&Window, With<PrimaryWindow>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<ViewportPanel>>,
    mut cursor: ResMut<TransformGizmoCursor>,
) {
    let Some(pos) = window.cursor_position() else {
        cursor.0 = Some(Vec2::splat(-10000.0));
        return;
    };
    let Ok((node, global)) = panels.single() else {
        cursor.0 = Some(Vec2::splat(-10000.0));
        return;
    };
    let rect = Rect::from_center_size(global.translation.trunc(), node.size());
    let physical = pos * window.scale_factor();
    cursor.0 = if physical.x >= rect.min.x
        && physical.y >= rect.min.y
        && physical.x <= rect.max.x
        && physical.y <= rect.max.y
    {
        Some(physical - rect.min)
    } else {
        Some(Vec2::splat(-10000.0))
    };
}

/// Paints the active mode onto its toolbar button and the mode label.
fn sync_toolbar(
    settings: Res<TransformGizmoSettings>,
    theme: Res<UiTheme>,
    mut buttons: Query<(&ToolButton, &mut BackgroundColor)>,
    mut labels: Query<&mut Text, With<ToolModeText>>,
) {
    let active = theme.color(&tokens::ACCENT);
    for (button, mut bg) in &mut buttons {
        let want = if button.0 == settings.mode {
            active
        } else {
            Color::NONE
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    let label = mode_label(settings.mode);
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.into();
        }
    }
}

/// The toolbar button for a mode: icon plus the accent when it is the live one.
pub fn tool_button(icon: Part, mode: TransformGizmoMode) -> Part {
    Box::new(bsn! {
        Node {
            width: px(26),
            height: px(26),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        BackgroundColor(Color::NONE)
        Button
        ToolButton(mode)
        Children [ {icon} ]
        on(tool_click)
    })
}

fn tool_click(
    click: On<PointerClick>,
    buttons: Query<&ToolButton>,
    parents: Query<&ChildOf>,
    mut settings: ResMut<TransformGizmoSettings>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.entity;
    let mut mode = None;
    loop {
        if let Ok(button) = buttons.get(entity) {
            mode = Some(button.0);
            break;
        }
        let Ok(parent) = parents.get(entity) else {
            break;
        };
        entity = parent.parent();
    }
    let Some(mode) = mode else {
        return;
    };
    if settings.mode != mode {
        settings.mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    #[test]
    fn translation_delta_carries_to_a_second_object() {
        let start = ObjectTransform::from_translation([2.0, 0.0, 0.0]);
        let from = Transform::from_xyz(1.0, 0.0, 0.0);
        let to = Transform::from_xyz(1.0, 0.0, 3.0);
        let got = moved_transform(&start, &from, &to);
        assert_eq!(got.translation, [2.0, 0.0, 3.0]);
        assert_eq!(got.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn rotation_and_scale_deltas_carry_to_a_second_object() {
        let start = ObjectTransform::default();
        let from = Transform::IDENTITY;
        let mut to = Transform::IDENTITY;
        to.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        to.scale = Vec3::splat(2.0);

        let got = moved_transform(&start, &from, &to);
        let want = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        assert!(Quat::from_array(got.rotation).abs_diff_eq(want, 1e-5));
        assert_eq!(got.scale, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn delta_lands_in_the_doc_of_every_other_selected_object() {
        let mut state = SceneState::default();
        let a = state.spawn(ObjectKind::Cube);
        let b = state.spawn(ObjectKind::Cube);
        state.select_many(&[a, b]);

        let start = state.doc.get(b).unwrap().transform();
        let before = state.doc.get(a).unwrap().translation;
        let from = Transform::from_xyz(0.0, 0.5, 0.0);
        let to = Transform::from_xyz(0.0, 2.0, 0.0);

        apply_to_selection(&mut state, &from, &to, &[(b, start)]);

        let got = state.doc.get(b).unwrap().translation;
        assert_eq!(
            got,
            [
                start.translation[0],
                start.translation[1] + 1.5,
                start.translation[2]
            ]
        );
        assert_eq!(state.doc.get(a).unwrap().translation, before);
    }
}
