//! Editor side of the engine's transform gizmo: focus follows the selection,
//! edits land back in the document, and the toolbar and hotkeys drive the mode.

use eme::feathers::theme::UiTheme;
use eme::gizmos::transform_gizmo::{
    TransformGizmoCursor, TransformGizmoFocus, TransformGizmoMode, TransformGizmoPlugin,
    TransformGizmoSettings, TransformGizmoSpace, TransformGizmoState, TransformGizmoSystems,
};
use eme::input_focus::InputFocus;
use eme::math::Rect;
use eme::picking::hover::Hovered;
use eme::prelude::*;
use eme::text::EditableText;
use eme::ui::BackgroundColor;
use eme::ui_widgets::Button;
use eme::window::{PrimaryWindow, Window};

use super::viewport::ViewportPanel;

use super::chrome::Part;
use crate::orbit::ViewMode;
use crate::scene::{propagate_edits, SceneLink, SceneState};
use crate::tokens;
use crate::undo::Undo;
use eme::math::ops;
use eme_game_scene::ObjectTransform;

/// Marks the toolbar button that switches to a gizmo mode.
#[derive(Component, Clone, Copy, Default)]
pub struct ToolButton(pub TransformGizmoMode);

/// The preferences the non-mode toolbar buttons toggle.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolbarToggleKind {
    /// Hide the gizmo without leaving the mode, or show it again.
    #[default]
    Select,
    /// Snap handles to fixed increments.
    Snap,
    /// Use world axes or the object's local axes.
    Space,
}

/// Marks a toolbar button that toggles a gizmo preference.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ToolbarToggle(pub ToolbarToggleKind);

/// Marks the label that names the active gizmo mode.
#[derive(Component, Clone, Copy, Default)]
pub struct ToolModeText;

/// Whether the transform gizmo is attached to the selection.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GizmoVisible(pub bool);

impl Default for GizmoVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Toolbar button box side, matching the mode buttons.
const TOOL_BUTTON: f32 = 26.0;
/// Translation increment while snapping is on.
const SNAP_TRANSLATE: f32 = 0.5;
/// Rotation increment while snapping is on, fifteen degrees in radians.
const SNAP_ROTATE: f32 = core::f32::consts::PI / 12.0;
/// Scale increment while snapping is on.
const SNAP_SCALE: f32 = 0.1;

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
            .init_resource::<GizmoVisible>()
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
    mut visible: ResMut<GizmoVisible>,
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
    if settings.mode != mode || !visible.0 {
        choose_mode(&mut settings, &mut visible, mode);
    }
}

fn choose_mode(
    settings: &mut TransformGizmoSettings,
    visible: &mut GizmoVisible,
    mode: TransformGizmoMode,
) {
    settings.mode = mode;
    visible.0 = true;
}

/// Moves the gizmo focus onto the primary selection, and off the previous one.
///
/// The engine gizmo runs on a `Single` focus entity: a second one silently
/// disables hover and the gizmo mesh, so only the primary ever carries it.
fn sync_focus(
    mut commands: Commands,
    state: Res<SceneState>,
    view: Res<ViewMode>,
    visible: Res<GizmoVisible>,
    mut tag: ResMut<FocusTag>,
) {
    let want = if *view == ViewMode::Game || !visible.0 {
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

/// Writes the gizmo's delta into every non-primary selected object's document,
/// recording each transform as it stood before the write.
fn apply_to_selection(
    state: &mut SceneState,
    from: &Transform,
    to: &Transform,
    others: &[(u64, ObjectTransform)],
    written: &mut Vec<(u64, ObjectTransform)>,
) {
    for (id, start) in others {
        let want = moved_transform(start, from, to);
        let Some(obj) = state.doc.get_mut(*id) else {
            continue;
        };
        let before = obj.transform();
        if obj.translation != want.translation {
            obj.translation = want.translation;
        }
        if obj.rotation != want.rotation {
            obj.rotation = want.rotation;
        }
        if obj.scale != want.scale {
            obj.scale = want.scale;
        }
        written.push((*id, before));
    }
}

/// Mirrors the focused entity back into the document after the gizmo moved it,
/// and carries the same delta onto the rest of the selection.
///
/// The document stays the source of truth: the entity is only a projection of it,
/// so a drag has to be written back before the next reconcile overwrites the entity.
/// Every id written here then carries its own change onto its descendants.
fn sync_doc(
    mut state: ResMut<SceneState>,
    gizmo: Res<TransformGizmoState>,
    focused: Query<(&SceneLink, &Transform), With<TransformGizmoFocus>>,
    mut undo: ResMut<Undo>,
    mut was_active: Local<bool>,
    mut others: Local<Vec<(u64, ObjectTransform)>>,
    mut written: Local<Vec<(u64, ObjectTransform)>>,
) {
    written.clear();
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

    if !gizmo.active && !was {
        return;
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
    let before = obj.transform();
    if obj.translation != translation {
        obj.translation = translation;
    }
    if obj.rotation != rotation {
        obj.rotation = rotation;
    }
    if obj.scale != scale {
        obj.scale = scale;
    }
    written.push((link.id, before));

    if gizmo.active {
        apply_to_selection(
            &mut state,
            &gizmo.start_transform,
            tf,
            &others,
            &mut written,
        );
    }

    propagate_edits(&mut state.doc, &written);

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

/// Paints the live toggle on each toolbar button and names the active mode.
fn sync_toolbar(
    settings: Res<TransformGizmoSettings>,
    visible: Res<GizmoVisible>,
    theme: Res<UiTheme>,
    mut modes: Query<(&ToolButton, &mut BackgroundColor, &Hovered)>,
    mut toggles: Query<(&ToolbarToggle, &mut BackgroundColor, &Hovered), Without<ToolButton>>,
    mut labels: Query<&mut Text, With<ToolModeText>>,
) {
    for (button, mut bg, hover) in &mut modes {
        let active = visible.0 && button.0 == settings.mode;
        paint(&mut bg, &theme, active, hover.get());
    }
    for (toggle, mut bg, hover) in &mut toggles {
        let active = match toggle.0 {
            ToolbarToggleKind::Select => !visible.0,
            ToolbarToggleKind::Snap => snapping(&settings),
            ToolbarToggleKind::Space => settings.space == TransformGizmoSpace::Local,
        };
        paint(&mut bg, &theme, active, hover.get());
    }
    let label = mode_label(settings.mode);
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.into();
        }
    }
}

fn paint(bg: &mut BackgroundColor, theme: &UiTheme, active: bool, hovered: bool) {
    let want = if active {
        theme.color(&tokens::ACCENT)
    } else if hovered {
        theme.color(&tokens::PANEL)
    } else {
        theme.color(&tokens::CLEAR)
    };
    if bg.0 != want {
        bg.0 = want;
    }
}

fn snapping(settings: &TransformGizmoSettings) -> bool {
    settings.snap_translate.is_some()
}

fn toggle_snap(settings: &mut TransformGizmoSettings) {
    if snapping(settings) {
        settings.snap_translate = None;
        settings.snap_rotate = None;
        settings.snap_scale = None;
    } else {
        settings.snap_translate = Some(SNAP_TRANSLATE);
        settings.snap_rotate = Some(SNAP_ROTATE);
        settings.snap_scale = Some(SNAP_SCALE);
    }
}

fn toggle_space(settings: &mut TransformGizmoSettings) {
    settings.space = match settings.space {
        TransformGizmoSpace::World => TransformGizmoSpace::Local,
        TransformGizmoSpace::Local => TransformGizmoSpace::World,
    };
}

/// The toolbar button for a mode: icon plus the accent when it is the live one.
pub fn tool_button(icon: Part, mode: TransformGizmoMode) -> Part {
    Box::new(bsn! {
        Node {
            width: px(TOOL_BUTTON),
            height: px(TOOL_BUTTON),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        BackgroundColor(Color::NONE)
        Button
        Hovered
        ToolButton(mode)
        Children [ {icon} ]
        on(tool_click)
    })
}

/// A toolbar button for a gizmo preference, with the same shell as a mode button.
pub fn toolbar_button(icon: Part, toggle: ToolbarToggleKind) -> Part {
    Box::new(bsn! {
        Node {
            width: px(TOOL_BUTTON),
            height: px(TOOL_BUTTON),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        BackgroundColor(Color::NONE)
        Button
        Hovered
        ToolbarToggle(toggle)
        Children [ {icon} ]
        on(toggle_click)
    })
}

fn nearest<T: Component + Copy>(
    mut entity: Entity,
    markers: &Query<&T>,
    parents: &Query<&ChildOf>,
) -> Option<T> {
    loop {
        if let Ok(marker) = markers.get(entity) {
            return Some(*marker);
        }
        let parent = parents.get(entity).ok()?;
        entity = parent.parent();
    }
}

fn tool_click(
    click: On<PointerClick>,
    buttons: Query<&ToolButton>,
    parents: Query<&ChildOf>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut visible: ResMut<GizmoVisible>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(mode) = nearest(click.entity, &buttons, &parents) else {
        return;
    };
    choose_mode(&mut settings, &mut visible, mode.0);
}

fn toggle_click(
    click: On<PointerClick>,
    buttons: Query<&ToolbarToggle>,
    parents: Query<&ChildOf>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut visible: ResMut<GizmoVisible>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some(toggle) = nearest(click.entity, &buttons, &parents) else {
        return;
    };
    match toggle.0 {
        ToolbarToggleKind::Select => visible.0 = !visible.0,
        ToolbarToggleKind::Snap => toggle_snap(&mut settings),
        ToolbarToggleKind::Space => toggle_space(&mut settings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    #[test]
    fn an_idle_gizmo_never_writes_the_entity_back_into_the_document() {
        let mut app = App::new();
        app.init_resource::<SceneState>()
            .init_resource::<Undo>()
            .init_resource::<TransformGizmoState>()
            .add_systems(Update, sync_doc);

        let id = {
            let mut state = app.world_mut().resource_mut::<SceneState>();
            let id = state.spawn(ObjectKind::Cube);
            state.select_only(id);
            state.doc.set_translation(id, [1.0, 2.0, 3.0]);
            id
        };

        let entity = app
            .world_mut()
            .spawn((
                SceneLink {
                    id,
                    kind: ObjectKind::Cube,
                },
                Transform::from_xyz(9.0, 9.0, 9.0),
                TransformGizmoFocus,
            ))
            .id();
        app.world_mut()
            .resource_mut::<SceneState>()
            .entities
            .insert(id, entity);

        app.update();

        let translation = app
            .world()
            .resource::<SceneState>()
            .doc
            .get(id)
            .unwrap()
            .translation;
        assert_eq!(
            translation,
            [1.0, 2.0, 3.0],
            "an idle gizmo copied the stale entity transform over the document"
        );
    }

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

        apply_to_selection(&mut state, &from, &to, &[(b, start)], &mut Vec::new());

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

    #[test]
    fn snap_toggle_sets_and_clears_exactly_three_increments() {
        let mut settings = TransformGizmoSettings::default();
        assert!(!snapping(&settings));

        toggle_snap(&mut settings);
        assert_eq!(settings.snap_translate, Some(SNAP_TRANSLATE));
        assert_eq!(settings.snap_rotate, Some(SNAP_ROTATE));
        assert_eq!(settings.snap_scale, Some(SNAP_SCALE));
        assert!(snapping(&settings));

        toggle_snap(&mut settings);
        assert_eq!(settings.snap_translate, None);
        assert_eq!(settings.snap_rotate, None);
        assert_eq!(settings.snap_scale, None);
        assert!(!snapping(&settings));
    }

    #[test]
    fn space_toggle_round_trips_world_local_world() {
        let mut settings = TransformGizmoSettings::default();
        assert_eq!(settings.space, TransformGizmoSpace::World);

        toggle_space(&mut settings);
        assert_eq!(settings.space, TransformGizmoSpace::Local);

        toggle_space(&mut settings);
        assert_eq!(settings.space, TransformGizmoSpace::World);
    }

    #[test]
    fn choosing_a_mode_restores_a_hidden_gizmo() {
        let mut settings = TransformGizmoSettings::default();
        let mut visible = GizmoVisible(false);

        choose_mode(&mut settings, &mut visible, TransformGizmoMode::Rotate);

        assert_eq!(settings.mode, TransformGizmoMode::Rotate);
        assert!(visible.0);
    }

    #[test]
    fn hiding_the_gizmo_detaches_focus_and_showing_restores_it() {
        let mut app = App::new();
        app.init_resource::<SceneState>()
            .init_resource::<ViewMode>()
            .init_resource::<GizmoVisible>()
            .init_resource::<FocusTag>()
            .add_systems(Update, sync_focus);

        let entity = app.world_mut().spawn_empty().id();
        {
            let mut state = app.world_mut().resource_mut::<SceneState>();
            let id = state.spawn(ObjectKind::Cube);
            state.entities.insert(id, entity);
            state.select_only(id);
        }

        app.update();
        assert!(app.world().entity(entity).contains::<TransformGizmoFocus>());

        app.world_mut().resource_mut::<GizmoVisible>().0 = false;
        app.update();
        assert!(!app.world().entity(entity).contains::<TransformGizmoFocus>());

        app.world_mut().resource_mut::<GizmoVisible>().0 = true;
        app.update();
        assert!(app.world().entity(entity).contains::<TransformGizmoFocus>());
    }
}
