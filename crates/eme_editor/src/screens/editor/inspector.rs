//! The Inspector dock and the Node/History dock beneath it.

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken};
use eme::input::keyboard::KeyboardInput;
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, FocusLost, FocusedInput, InputFocus};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextEdit, TextFont,
    TextLayout,
};
use eme::ui_widgets::{Activate, Button, TextInput};
use eme_game_overrides::{ModelNodeCatalog, ModelOverrideStatus, OrphanReason};
use eme_game_scene::{
    Behaviour, BehaviourKind, NodeKey, OverrideOp, ObjectTransform,
};
use std::path::{Path, PathBuf};

use super::chrome::*;
use super::docks;
use super::icons;
use crate::scene::{self, ModelInstance, ModelOwner, NodeSelection, SceneState};
use crate::tokens;
use crate::undo::Undo;

/// Smallest scale magnitude the inspector accepts, matching the gizmo clamp.
const MIN_SCALE: f32 = 0.01;

/// Which transform row an inspector field reflects.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    Position,
    Rotation,
    Scale,
}

/// Marks a transform value field so the scene can keep it live.
#[derive(Component, Clone, Copy)]
pub struct AxisValue(pub Prop, pub u8);

/// Remembers which object a focused transform field was opened on.
#[derive(Component, Clone, Copy)]
pub struct FieldEdit {
    target: u64,
}

/// Marks the inspector header label so it shows the selected object.
#[derive(Component, Clone, Default)]
pub struct ObjectTitle;

/// Wrapper hidden unless a model node is selected.
#[derive(Component, Clone, Default)]
pub struct NodeSectionRoot;

/// Node name inside the model-node section.
#[derive(Component, Clone, Default)]
pub struct NodeTitle;

/// Node visibility value text.
#[derive(Component, Clone, Default)]
pub struct NodeVisibleText;

/// Button that flips the selected node's visibility override.
#[derive(Component, Clone, Copy, Default)]
pub struct NodeVisibleButton;

/// One editable field in the model-node transform.
#[derive(Component, Clone, Copy)]
pub struct NodeAxisValue(pub Prop, pub u8);

/// Remembers which node a focused node field was opened on.
#[derive(Component, Clone)]
pub struct NodeFieldEdit(pub u64, pub NodeKey);

/// Host for orphan and diff rows.
#[derive(Component, Clone, Default)]
pub struct ModelOverrideRows;

/// Remove one orphaned override.
#[derive(Component, Clone, Default)]
pub struct RemoveOrphan(pub u64, pub NodeKey);

/// Expand the node choices for an orphan.
#[derive(Component, Clone, Default)]
pub struct ReattachOrphan(pub u64, pub NodeKey);

/// Point an orphan at this node.
#[derive(Component, Clone, Default)]
pub struct AttachCandidate(pub u64, pub NodeKey, pub NodeKey);

/// Orphan currently choosing a new node.
#[derive(Resource, Clone, Default, Debug, PartialEq)]
pub struct ReattachState {
    /// Scene object that owns the model.
    pub object: Option<u64>,
    /// Orphaned key being reattached.
    pub key: Option<NodeKey>,
}

impl ReattachState {
    /// Forgets the pending reattach.
    pub fn clear(&mut self) {
        self.object = None;
        self.key = None;
    }
}

/// Marks the behaviour row's value text so it follows the selection.
#[derive(Component, Clone, Copy, Default)]
pub struct BehaviourValue;

/// An inspector section whose body can be collapsed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    Transform,
    Behaviour,
    Visibility,
    Script,
    Node,
}

/// Which inspector sections are open. All sections start open.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct InspectorSections {
    pub transform: bool,
    pub behaviour: bool,
    pub visibility: bool,
    pub script: bool,
    pub node: bool,
}

impl Default for InspectorSections {
    fn default() -> Self {
        Self {
            transform: true,
            behaviour: true,
            visibility: true,
            script: true,
            node: true,
        }
    }
}

impl InspectorSections {
    /// Whether the body of `section` is visible.
    pub fn is_open(&self, section: Section) -> bool {
        match section {
            Section::Transform => self.transform,
            Section::Behaviour => self.behaviour,
            Section::Visibility => self.visibility,
            Section::Script => self.script,
            Section::Node => self.node,
        }
    }

    /// Shows or hides the body of `section`.
    pub fn set_open(&mut self, section: Section, open: bool) {
        match section {
            Section::Transform => self.transform = open,
            Section::Behaviour => self.behaviour = open,
            Section::Visibility => self.visibility = open,
            Section::Script => self.script = open,
            Section::Node => self.node = open,
        }
    }

    /// Flips `section` between open and closed.
    pub fn toggle(&mut self, section: Section) {
        let open = !self.is_open(section);
        self.set_open(section, open);
    }
}

/// Marks a section header row; activating it toggles the section.
#[derive(Component, Clone, Copy, Default)]
pub struct SectionHeader(pub Section);

/// Marks the container holding a section's body rows.
#[derive(Component, Clone, Copy, Default)]
pub struct SectionBody(pub Section);

/// One of the two carets a section header shows; `true` is the open glyph.
#[derive(Component, Clone, Copy, Default)]
pub struct SectionCaret(pub Section, pub bool);

/// Inspector panel body: the object header and the property sections.
pub fn body() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: px(2),
                padding: UiRect {
                    left: px(10),
                    right: px(8),
                    top: px(4),
                    bottom: px(4),
                },
            }
            ImportReportRows
            --
            {object_row()}
            --
            {node_section()}
            --
            {override_rows()}
            --
            {section(Section::Transform, "Transform", transform_rows())}
            --
            {section(Section::Behaviour, "Behaviour", vec![behaviour_row()])}
            --
            {section(Section::Visibility, "Visibility", vec![visibility_row()])}
            --
            {section(Section::Script, "Script", vec![script_row()])}
            --
            {spacer()}
        ]
    })
}

fn object_row() -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            border: UiRect { bottom: px(1) },
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {icon_button(icons::lock(), 18.0)}
            --
            {icons::badge(icons::NODE)}
            --
            {title_text("Player".into())}
            --
            {spacer()}
            --
            {icon_button(icons::dock_menu(), 20.0)}
        ]
    })
}

fn title_text(value: String) -> Part {
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        ObjectTitle
    })
}

fn value_text(value: String, seat: AxisValue) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(value)
        TextInput
        TabIndex(0)
        TextLayout {
            linebreak: LineBreak::NoWrap,
        }
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        seat
        on(focus_field)
        on(blur_field)
        on(field_key)
    })
}

/// Gives a clicked transform field the keyboard and selects its current value.
fn focus_field(
    press: On<PointerPress>,
    fields: Query<&AxisValue>,
    state: Res<SceneState>,
    mut values: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    let Some(target) = state.primary() else {
        return;
    };
    commands.entity(press.entity).insert(FieldEdit { target });
    if let Ok(mut value) = values.get_mut(press.entity) {
        value.queue_edit(TextEdit::SelectAll);
    }
    focus.set(press.entity, FocusCause::Pressed);
}

/// Commits on Enter, reverts on Escape.
fn field_key(
    key: On<FocusedInput<KeyboardInput>>,
    fields: Query<(&AxisValue, &EditableText)>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let input = &key.input;
    if !input.state.is_pressed() || input.repeat {
        return;
    }
    let entity = key.focused_entity;
    let Ok((seat, value)) = fields.get(entity) else {
        return;
    };
    if input.key_code == KeyCode::Escape {
        commands.entity(entity).remove::<FieldEdit>();
        return;
    }
    if input.key_code != KeyCode::Enter {
        return;
    }
    commit_field(
        &mut state,
        &mut undo,
        *seat,
        value.value().to_string().as_str(),
    );
    commands.entity(entity).remove::<FieldEdit>();
    focus.clear();
}

/// Commits a field that lost focus, unless the primary selection moved away.
fn blur_field(
    lost: On<FocusLost>,
    edits: Query<&FieldEdit>,
    fields: Query<(&AxisValue, &EditableText)>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut commands: Commands,
) {
    let entity = lost.event_target();
    let Ok(edit) = edits.get(entity) else {
        return;
    };
    commands.entity(entity).remove::<FieldEdit>();
    if state.primary() != Some(edit.target) {
        return;
    }
    let Ok((seat, value)) = fields.get(entity) else {
        return;
    };
    commit_field(
        &mut state,
        &mut undo,
        *seat,
        value.value().to_string().as_str(),
    );
}

fn node_entity(
    object: u64,
    key: &NodeKey,
    catalogs: &Query<(&ModelOwner, &ModelNodeCatalog)>,
) -> Option<Entity> {
    catalogs
        .iter()
        .find(|(owner, _)| owner.0 == object)
        .and_then(|(_, catalog)| catalog.nodes.iter().find(|node| &node.key == key))
        .and_then(|node| node.entity)
}

fn node_transform(
    state: &SceneState,
    object: u64,
    key: &NodeKey,
    catalogs: &Query<(&ModelOwner, &ModelNodeCatalog)>,
    transforms: &Query<&Transform>,
) -> Option<ObjectTransform> {
    if let Some(entity) = node_entity(object, key, catalogs)
        && let Ok(transform) = transforms.get(entity)
    {
        return Some(ObjectTransform {
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
        });
    }
    state.doc.overrides(object).iter().find_map(|entry| {
        if entry.key == *key
            && let OverrideOp::SetTransform(transform) = &entry.op
        {
            return Some(*transform);
        }
        None
    })
}

fn node_visible(
    state: &SceneState,
    object: u64,
    key: &NodeKey,
    catalogs: &Query<(&ModelOwner, &ModelNodeCatalog)>,
    visibilities: &Query<&Visibility>,
) -> bool {
    if let Some(value) = state.doc.overrides(object).iter().find_map(|entry| {
        if entry.key == *key
            && let OverrideOp::SetVisible { visible } = entry.op
        {
            return Some(visible);
        }
        None
    }) {
        return value;
    }
    node_entity(object, key, catalogs)
        .and_then(|entity| visibilities.get(entity).ok())
        .is_none_or(|visibility| !matches!(visibility, Visibility::Hidden))
}

/// Gives a clicked node field the keyboard and remembers which node it edits.
fn focus_node_field(
    press: On<PointerPress>,
    fields: Query<&NodeAxisValue>,
    node: Res<NodeSelection>,
    mut values: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    let (Some(object), Some(key)) = (node.object, node.key.clone()) else {
        return;
    };
    commands.entity(press.entity).insert(NodeFieldEdit(object, key));
    if let Ok(mut value) = values.get_mut(press.entity) {
        value.queue_edit(TextEdit::SelectAll);
    }
    focus.set(press.entity, FocusCause::Pressed);
}

/// Commits a node field on Enter, reverts on Escape.
fn node_field_key(
    key: On<FocusedInput<KeyboardInput>>,
    fields: Query<(&NodeAxisValue, &EditableText)>,
    edits: Query<&NodeFieldEdit>,
    catalogs: Query<(&ModelOwner, &ModelNodeCatalog)>,
    transforms: Query<&Transform>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let input = &key.input;
    if !input.state.is_pressed() || input.repeat {
        return;
    }
    let entity = key.focused_entity;
    let Ok((seat, value)) = fields.get(entity) else {
        return;
    };
    if input.key_code == KeyCode::Escape {
        commands.entity(entity).remove::<NodeFieldEdit>();
        return;
    }
    if input.key_code != KeyCode::Enter {
        return;
    }
    if let Ok(edit) = edits.get(entity) {
        commit_node_field(
            &mut state,
            &mut undo,
            *seat,
            value.value().to_string().as_str(),
            edit,
            &catalogs,
            &transforms,
        );
    }
    commands.entity(entity).remove::<NodeFieldEdit>();
    focus.clear();
}

/// Commits a node field that lost focus.
fn blur_node_field(
    lost: On<FocusLost>,
    edits: Query<&NodeFieldEdit>,
    fields: Query<(&NodeAxisValue, &EditableText)>,
    catalogs: Query<(&ModelOwner, &ModelNodeCatalog)>,
    transforms: Query<&Transform>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut commands: Commands,
) {
    let entity = lost.event_target();
    let Ok(edit) = edits.get(entity) else {
        return;
    };
    commands.entity(entity).remove::<NodeFieldEdit>();
    let Ok((seat, value)) = fields.get(entity) else {
        return;
    };
    commit_node_field(
        &mut state,
        &mut undo,
        *seat,
        value.value().to_string().as_str(),
        edit,
        &catalogs,
        &transforms,
    );
}

/// Parses a node field and stores it as a transform override with one checkpoint.
fn commit_node_field(
    state: &mut SceneState,
    undo: &mut Undo,
    seat: NodeAxisValue,
    raw: &str,
    edit: &NodeFieldEdit,
    catalogs: &Query<(&ModelOwner, &ModelNodeCatalog)>,
    transforms: &Query<&Transform>,
) -> bool {
    let Some(value) = parse_num(raw) else {
        return false;
    };
    let Some(before) = node_transform(state, edit.0, &edit.1, catalogs, transforms) else {
        return false;
    };
    let axis = seat.1 as usize;
    if axis >= 3 {
        return false;
    }
    let (label, want) = match seat.0 {
        Prop::Position => {
            let mut translation = before.translation;
            translation[axis] = value;
            (
                "Edit Node Position",
                ObjectTransform {
                    translation,
                    ..before
                },
            )
        }
        Prop::Rotation => {
            let mut angles = degrees(before.rotation);
            angles[axis] = value;
            let rotation = euler_quat(angles);
            ("Edit Node Rotation", ObjectTransform { rotation, ..before })
        }
        Prop::Scale => {
            let Some(scale) = clamp_scale(value) else {
                return false;
            };
            let mut axes = before.scale;
            axes[axis] = scale;
            ("Edit Node Scale", ObjectTransform { scale: axes, ..before })
        }
    };
    if want == before {
        return false;
    }
    undo.checkpoint(state, label);
    state.doc
        .set_override(edit.0, edit.1.clone(), OverrideOp::SetTransform(want));
    state.touch();
    true
}

/// Flips the selected node's visibility override.
fn toggle_node_visible(
    _: On<Activate>,
    node: Res<NodeSelection>,
    catalogs: Query<(&ModelOwner, &ModelNodeCatalog)>,
    visibilities: Query<&Visibility>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
) {
    let (Some(object), Some(key)) = (node.object, node.key.clone()) else {
        return;
    };
    let current = node_visible(&state, object, &key, &catalogs, &visibilities);
    undo.checkpoint(&state, "Toggle Node Visibility");
    state.doc.set_override(
        object,
        key,
        OverrideOp::SetVisible {
            visible: !current,
        },
    );
    state.touch();
}

/// Parses one field and writes it to the primary object, with one undo checkpoint.
///
/// Only the primary object is edited; the rest of a multi-selection is left
/// untouched. Returns false when the text does not parse, the primary is gone,
/// or the value would not change the document.
fn commit_field(state: &mut SceneState, undo: &mut Undo, seat: AxisValue, raw: &str) -> bool {
    let Some(value) = parse_num(raw) else {
        return false;
    };
    let Some(id) = state.primary() else {
        return false;
    };
    let Some(before) = state
        .doc
        .get(id)
        .map(eme_game_scene::SceneObject::transform)
    else {
        return false;
    };
    let axis = seat.1 as usize;
    if axis >= 3 {
        return false;
    }
    let (label, want) = match seat.0 {
        Prop::Position => {
            let mut translation = before.translation;
            translation[axis] = value;
            (
                "Edit Position",
                ObjectTransform {
                    translation,
                    ..before
                },
            )
        }
        Prop::Rotation => {
            let mut angles = degrees(before.rotation);
            angles[axis] = value;
            let rotation = euler_quat(angles);
            ("Edit Rotation", ObjectTransform { rotation, ..before })
        }
        Prop::Scale => {
            let Some(scale) = clamp_scale(value) else {
                return false;
            };
            let mut axes = before.scale;
            axes[axis] = scale;
            (
                "Edit Scale",
                ObjectTransform {
                    scale: axes,
                    ..before
                },
            )
        }
    };
    if want == before {
        return false;
    }
    undo.checkpoint(state, label);
    match seat.0 {
        Prop::Position => {
            state.doc.set_translation(id, want.translation);
        }
        Prop::Rotation => {
            state.doc.set_rotation(id, want.rotation);
        }
        Prop::Scale => {
            state.doc.set_scale(id, want.scale);
        }
    }
    scene::propagate(&mut state.doc, id, before, &[]);
    state.touch();
    true
}

fn parse_num(raw: &str) -> Option<f32> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let value: f32 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

fn clamp_scale(value: f32) -> Option<f32> {
    if value == 0.0 || !value.is_finite() {
        return None;
    }
    let mag = value.abs().max(MIN_SCALE);
    Some(if value < 0.0 { -mag } else { mag })
}

fn euler_quat(angles: [f32; 3]) -> [f32; 4] {
    Quat::from_euler(
        EulerRot::XYZ,
        angles[0].to_radians(),
        angles[1].to_radians(),
        angles[2].to_radians(),
    )
    .to_array()
}

fn section(id: Section, title: &str, rows: Vec<Part>) -> Part {
    let body = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        SectionBody(id)
        Children [ {rows} ]
    }) as Part;

    col(0.0, vec![section_header(id, title), body])
}

fn section_header(id: Section, title: &str) -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::CATEGORY)
        Button
        SectionHeader(id)
        Children [
            {caret_slot(id, true)}
            --
            {caret_slot(id, false)}
            --
            {text(title, FONT_SIZE, tokens::SECTION_TEXT)}
        ]
        on(toggle_section)
    })
}

fn caret_slot(id: Section, open: bool) -> Part {
    let glyph = icons::caret(open);
    let display = if open { Display::Flex } else { Display::None };
    Box::new(bsn! {
        SectionCaret(id, open)
        Node { display: {display} }
        Children [ {glyph} ]
    })
}

/// Flips the section whose header was activated.
fn toggle_section(
    activate: On<Activate>,
    headers: Query<&SectionHeader>,
    mut sections: Option<ResMut<InspectorSections>>,
) {
    let Ok(header) = headers.get(activate.entity) else {
        return;
    };
    if let Some(sections) = sections.as_mut() {
        sections.toggle(header.0);
    }
}

fn transform_rows() -> Vec<Part> {
    vec![
        property(
            "Position",
            Prop::Position,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        property(
            "Rotation",
            Prop::Rotation,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        property(
            "Scale",
            Prop::Scale,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "1"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "1"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "1"),
            ],
        ),
    ]
}

/// Container hidden unless a model node is selected.
fn node_section() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        NodeSectionRoot
        Children [
            {section(Section::Node, "Model Node", node_rows())}
        ]
    })
}

/// Host that the orphan and diff rows are rebuilt into.
fn override_rows() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(2),
        }
        ModelOverrideRows
    })
}

fn node_rows() -> Vec<Part> {
    vec![
        Box::new(bsn! {
            Node {
                height: px(PROPERTY_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
            }
            Children [ {node_title()} ]
        }) as Part,
        node_property(
            "Position",
            Prop::Position,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        node_property(
            "Rotation",
            Prop::Rotation,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        node_property(
            "Scale",
            Prop::Scale,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "1"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "1"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "1"),
            ],
        ),
        node_visible_row(),
    ]
}

fn node_title() -> Part {
    Box::new(bsn! {
        Node { flex_grow: 1.0, display: Display::Flex, align_items: AlignItems::Center }
        Children [ {node_title_text()} ]
    })
}

fn node_title_text() -> Part {
    Box::new(bsn! {
        Text("No node")
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        NodeTitle
    })
}

fn node_property(label: &str, prop: Prop, axes: [(&str, ThemeToken, ThemeToken, &str); 3]) -> Part {
    let mut fields = Vec::new();
    for (i, (letter, line, letter_tint, value)) in axes.into_iter().enumerate() {
        fields.push(node_axis_field(
            letter,
            line,
            letter_tint,
            value,
            NodeAxisValue(prop, i as u8),
        ));
    }

    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(4),
            },
            column_gap: px(6),
        }
        Children [
            {label_cell(label)}
            --
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
            }
            Children [ {fields} ]
        ]
    })
}

fn node_axis_field(
    letter: &str,
    line: ThemeToken,
    letter_tint: ThemeToken,
    value: &str,
    seat: NodeAxisValue,
) -> Part {
    let axis = text(letter, 9.0, letter_tint);
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(20),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(3)),
            border: UiRect { left: px(2) },
            column_gap: px(3),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::FIELD)
        ThemeBorderColor(line)
        Children [
            {axis}
            --
            {node_value_text(value.to_owned(), seat)}
        ]
    })
}

fn node_value_text(value: String, seat: NodeAxisValue) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(value)
        TextInput
        TabIndex(0)
        TextLayout {
            linebreak: LineBreak::NoWrap,
        }
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        seat
        on(focus_node_field)
        on(blur_node_field)
        on(node_field_key)
    })
}

fn node_visible_row() -> Part {
    let toggle = Box::new(bsn! {
        Node {
            height: px(22),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
        }
        Button
        NodeVisibleButton
        Children [ {node_visible_value()} ]
        on(toggle_node_visible)
    }) as Part;
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(6),
        }
        Children [
            {label_cell("Visible")}
            --
            {spacer()}
            --
            {toggle}
        ]
    })
}

fn node_visible_value() -> Part {
    Box::new(bsn! {
        Text("yes")
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::ACCENT)
        NodeVisibleText
    })
}

fn label_cell(label: &str) -> Part {
    Box::new(bsn! {
        Node { width: px(64) }
        Children [ {text(label, FONT_SIZE, tokens::PROP_TEXT)} ]
    })
}

fn property(label: &str, prop: Prop, axes: [(&str, ThemeToken, ThemeToken, &str); 3]) -> Part {
    let mut fields = Vec::new();
    for (i, (letter, line, letter_tint, value)) in axes.into_iter().enumerate() {
        fields.push(axis_field(
            letter,
            line,
            letter_tint,
            value,
            AxisValue(prop, i as u8),
        ));
    }

    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(4),
            },
            column_gap: px(6),
        }
        Children [
            {label_cell(label)}
            --
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
            }
            Children [ {fields} ]
        ]
    })
}

fn axis_field(
    letter: &str,
    line: ThemeToken,
    letter_tint: ThemeToken,
    value: &str,
    seat: AxisValue,
) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(FIELD_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            border: UiRect { bottom: px(2) },
            border_radius: px(3),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::FIELD)
        ThemeBorderColor(line)
        Children [
            Node {
                width: px(24),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            ThemeBackgroundColor(tokens::LABEL_STRIP)
            Children [ {text(letter, FONT_SIZE, letter_tint)} ]
            --
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect { left: px(6) },
            }
            Children [ {value_text(value.into(), seat)} ]
        ]
    })
}

fn behaviour_row() -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(6),
        }
        Button
        Children [
            {label_cell("Play")}
            --
            {field(vec![behaviour_text()], FIELD_H)}
        ]
        on(cycle_behaviour_row)
    })
}

fn behaviour_text() -> Part {
    let value = BehaviourKind::None.label().to_string();
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        BehaviourValue
    })
}

/// Advances the primary object through None → Spin → Bob, with one checkpoint.
fn cycle_behaviour(state: &mut SceneState, undo: &mut Undo) -> bool {
    let Some(id) = state.primary() else {
        return false;
    };
    let Some(current) = state.doc.get(id).map(|obj| obj.behaviour) else {
        return false;
    };
    let next = Behaviour {
        kind: current.kind.next(),
        ..current
    };
    undo.checkpoint(state, "Behaviour");
    state.doc.set_behaviour(id, next);
    state.touch();
    true
}

fn cycle_behaviour_row(_: On<Activate>, mut state: ResMut<SceneState>, mut undo: ResMut<Undo>) {
    cycle_behaviour(&mut state, &mut undo);
}

fn behaviour_label(spec: Behaviour) -> String {
    match spec.kind {
        BehaviourKind::None => BehaviourKind::None.label().to_string(),
        kind => format!("{} · {:.1}", kind.label(), spec.speed),
    }
}

fn visibility_row() -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(6),
        }
        Children [
            {label_cell("Visible")}
            --
            {spacer()}
            --
            {icons::check()}
        ]
    })
}

fn script_row() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(5),
            },
            column_gap: px(6),
        }
        Children [
            {label_cell("Script")}
            --
            {field(
                vec![
                    icons::file(),
                    text("Empty", FONT_SIZE, tokens::TEXT_DIM),
                ],
                FIELD_H,
            )}
        ]
    })
}

fn fmt_num(value: f32) -> String {
    let mut text = format!("{value:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

fn degrees(rotation: [f32; 4]) -> [f32; 3] {
    let (x, y, z) = Quat::from_array(rotation).to_euler(EulerRot::XYZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// Header text for a selection of `count` objects, named by its primary.
fn selection_title(count: usize, name: Option<&str>) -> String {
    match (count, name) {
        (0, _) | (1, None) => "No selection".into(),
        (1, Some(name)) => name.into(),
        (n, _) => format!("{n} Objects"),
    }
}

/// Keeps the inspector header and transform fields in step with the selection,
/// and the history dock beneath it in step with the undo stack.
///
/// While a field is being edited it is left alone; every other field keeps
/// repainting from the document, so a gizmo drag still updates them live.
pub fn sync_inspector(
    state: Res<SceneState>,
    undo: Res<Undo>,
    hosts: Query<Entity, With<docks::HistoryRows>>,
    mut commands: Commands,
    sections: Option<ResMut<InspectorSections>>,
    mut focus: ResMut<InputFocus>,
    mut values: Query<(Entity, &AxisValue, &mut EditableText, Option<&FieldEdit>)>,
    mut titles: Query<&mut Text, (With<ObjectTitle>, Without<BehaviourValue>)>,
    mut behaviours: Query<&mut Text, (With<BehaviourValue>, Without<ObjectTitle>)>,
    mut slots: Query<
        (&mut Node, Option<&SectionBody>, Option<&SectionCaret>),
        Or<(With<SectionBody>, With<SectionCaret>)>,
    >,
    mut last_history: Local<Option<Vec<&'static str>>>,
) {
    docks::sync_history(&undo, &hosts, &mut commands, &mut last_history);
    let open = match sections.as_deref() {
        Some(sections) => *sections,
        None => {
            commands.init_resource::<InspectorSections>();
            InspectorSections::default()
        }
    };

    for (mut node, body, caret) in &mut slots {
        let want = match (body, caret) {
            (Some(body), _) => open.is_open(body.0),
            (_, Some(caret)) => caret.1 == open.is_open(caret.0),
            _ => continue,
        };
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }

    let selected = state.selected_object();
    let primary = state.primary();

    for (entity, seat, mut value, edit) in &mut values {
        if let Some(edit) = edit
            && primary != Some(edit.target)
        {
            commands.entity(entity).remove::<FieldEdit>();
            if focus.get() == Some(entity) {
                focus.clear();
            }
            continue;
        }
        if edit.is_some() || focus.get() == Some(entity) {
            continue;
        }
        let text = match (selected, seat.0) {
            (Some(obj), Prop::Position) => fmt_num(obj.translation[seat.1 as usize]),
            (Some(obj), Prop::Rotation) => fmt_num(degrees(obj.rotation)[seat.1 as usize]),
            (Some(obj), Prop::Scale) => fmt_num(obj.scale[seat.1 as usize]),
            (None, Prop::Scale) => "1".into(),
            (None, _) => "0".into(),
        };
        if value.value().to_string() != text {
            value.editor.set_text(&text);
        }
    }

    let title = selection_title(state.selection.len(), selected.map(|obj| obj.name.as_str()));
    for mut text in &mut titles {
        if text.0 != title {
            text.0 = title.clone();
        }
    }

    let behaviour = selected.map_or_else(
        || BehaviourKind::None.label().to_string(),
        |obj| behaviour_label(obj.behaviour),
    );
    for mut text in &mut behaviours {
        if text.0 != behaviour {
            text.0 = behaviour.clone();
        }
    }
}

/// Keeps the model-node section in step with the node selection.
pub fn sync_node_inspector(
    node: Res<NodeSelection>,
    state: Res<SceneState>,
    catalogs: Query<(&ModelOwner, &ModelNodeCatalog)>,
    transforms: Query<&Transform>,
    visibilities: Query<&Visibility>,
    mut roots: Query<&mut Node, With<NodeSectionRoot>>,
    mut titles: Query<&mut Text, (With<NodeTitle>, Without<NodeVisibleText>)>,
    mut visibles: Query<&mut Text, (With<NodeVisibleText>, Without<NodeTitle>)>,
    mut fields: Query<(Entity, &NodeAxisValue, &mut EditableText, Option<&NodeFieldEdit>)>,
) {
    let selected = node
        .object
        .zip(node.key.clone())
        .filter(|_| state.doc.get(node.object.unwrap_or_default()).is_some());
    for mut root in &mut roots {
        let want = if selected.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if root.display != want {
            root.display = want;
        }
    }
    let Some((object, key)) = selected else {
        return;
    };

    let title = if key.path.is_empty() {
        key.name.clone()
    } else {
        let path = key.path.iter().map(u32::to_string).collect::<Vec<_>>().join(".");
        format!("{}  [{}]", key.name, path)
    };
    for mut text in &mut titles {
        if text.0 != title {
            text.0 = title.clone();
        }
    }
    let visible = node_visible(&state, object, &key, &catalogs, &visibilities);
    let label = if visible { "yes" } else { "no" };
    for mut text in &mut visibles {
        if text.0 != label {
            text.0 = label.into();
        }
    }

    let Some(transform) = node_transform(&state, object, &key, &catalogs, &transforms) else {
        return;
    };
    for (_entity, seat, mut value, edit) in &mut fields {
        if edit.is_some() {
            continue;
        }
        let text = match seat.0 {
            Prop::Position => fmt_num(transform.translation[seat.1 as usize]),
            Prop::Rotation => fmt_num(degrees(transform.rotation)[seat.1 as usize]),
            Prop::Scale => fmt_num(transform.scale[seat.1 as usize]),
        };
        if value.value().to_string() != text {
            value.editor.set_text(&text);
        }
    }
}

/// Finds the latest import diff for a live instance, matching files by real path.
fn latest_diff<'a>(
    service: &'a crate::import::ImportService,
    instance: &ModelInstance,
) -> Option<&'a crate::import::NodeDiff> {
    let want = std::fs::canonicalize(crate::asset_root().join(&instance.path)).ok()?;
    service.diffs.iter().find_map(|(path, diff)| {
        (std::fs::canonicalize(path).ok().as_deref() == Some(want.as_path())).then_some(diff)
    })
}

/// Rebuilds the diff and orphan rows for the selected model.
pub fn sync_override_panel(
    mut commands: Commands,
    state: Res<SceneState>,
    node: Res<NodeSelection>,
    service: Res<crate::import::ImportService>,
    roots: Query<(
        &ModelOwner,
        &ModelNodeCatalog,
        &ModelOverrideStatus,
        &ModelInstance,
    )>,
    mut reattach: ResMut<ReattachState>,
    hosts: Query<Entity, With<ModelOverrideRows>>,
    mut shown: Local<Option<(Entity, String)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    let object = node.object.or_else(|| state.primary());
    let model = object.and_then(|id| {
        roots
            .iter()
            .find(|(owner, ..)| owner.0 == id)
            .map(|(_, catalog, status, instance)| (catalog, status, instance))
    });
    if reattach
        .object
        .is_some_and(|pending| Some(pending) != object)
    {
        reattach.clear();
    }
    let signature = match model {
        Some((catalog, status, instance)) => {
            let diff = latest_diff(&service, instance).map_or_else(String::new, |diff| {
                let mut text = diff.headline();
                for line in diff.summary_lines() {
                    text.push('\n');
                    text.push_str(&line);
                }
                text
            });
            format!(
                "{:?}|{}|{}|{:?}|{diff}",
                object,
                status.revision,
                catalog.nodes.len(),
                reattach
            )
        }
        None => format!("{:?}|none", object),
    };
    if shown.as_ref() == Some(&(host, signature.clone())) {
        return;
    }
    *shown = Some((host, signature));
    commands.entity(host).despawn_children();
    let Some((catalog, status, instance)) = model else {
        return;
    };
    let Some(object) = object else {
        return;
    };

    if let Some(diff) = latest_diff(&service, instance) {
        let headline = text(
            format!("node diff: {}", diff.headline()),
            FONT_SIZE,
            tokens::TEXT_DIM,
        );
        commands.spawn_scene(bsn! {
            Node { display: Display::Flex, padding: UiRect::horizontal(px(8)) }
            ChildOf(host)
            Children [ {headline} ]
        });
        for line in diff.summary_lines() {
            let row = text(line, FONT_SIZE, tokens::TEXT_DIM);
            commands.spawn_scene(bsn! {
                Node {
                    display: Display::Flex,
                    padding: UiRect { left: px(14), right: px(8) },
                }
                ChildOf(host)
                Children [ {row} ]
            });
        }
    }

    for orphan in &status.orphans {
        let reason = match &orphan.reason {
            OrphanReason::Missing => "no node with this name".to_string(),
            OrphanReason::Renamed { found } => format!("path now holds \"{found}\""),
        };
        let label = format!("orphan {}: {reason}", orphan.key.name);
        let title = text(label, FONT_SIZE, tokens::TEXT_DIM);
        let remove_key = orphan.key.clone();
        let reattach_key = orphan.key.clone();
        let remove_label = text("Remove", FONT_SIZE, tokens::TEXT);
        let reattach_label = text("Reattach", FONT_SIZE, tokens::ACCENT);
        let remove = Box::new(bsn! {
            Node { padding: UiRect::horizontal(px(6)), height: px(22), display: Display::Flex, align_items: AlignItems::Center }
            Button
            RemoveOrphan(object, remove_key)
            Children [ {remove_label} ]
            on(remove_orphan)
        }) as Part;
        let reattach_button = Box::new(bsn! {
            Node { padding: UiRect::horizontal(px(6)), height: px(22), display: Display::Flex, align_items: AlignItems::Center }
            Button
            ReattachOrphan(object, reattach_key)
            Children [ {reattach_label} ]
            on(reattach_orphan)
        }) as Part;
        commands.spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(2),
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(host)
            Children [ {title} -- {spacer()} -- {reattach_button} -- {remove} ]
        });

        if reattach.object == Some(object) && reattach.key.as_ref() == Some(&orphan.key) {
            for candidate in &catalog.nodes {
                if candidate.key == orphan.key {
                    continue;
                }
                let from_key = orphan.key.clone();
                let to_key = candidate.key.clone();
                let candidate_label = text(
                    format!("{}  [{}]", candidate.key.name, candidate.key.path.iter().map(u32::to_string).collect::<Vec<_>>().join(".")),
                    FONT_SIZE,
                    tokens::TEXT_DIM,
                );
                commands
                    .spawn_scene(bsn! {
                        Node {
                            padding: UiRect { left: px(20), right: px(8), top: px(2), bottom: px(2) },
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                        }
                        Button
                        AttachCandidate(object, from_key, to_key)
                        ChildOf(host)
                        Children [ {candidate_label} ]
                        on(attach_candidate)
                    });
            }
        }
    }
}

/// Removes every override attached to an orphaned key.
fn remove_orphan(
    activate: On<Activate>,
    rows: Query<&RemoveOrphan>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    undo.checkpoint(&state, "Remove Override");
    if state.doc.remove_node_overrides(row.0, &row.1) {
        state.touch();
    }
}

/// Starts choosing a new node for an orphan.
fn reattach_orphan(
    activate: On<Activate>,
    rows: Query<&ReattachOrphan>,
    mut reattach: ResMut<ReattachState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    reattach.object = Some(row.0);
    reattach.key = Some(row.1.clone());
}

/// Points an orphan at the chosen node.
fn attach_candidate(
    activate: On<Activate>,
    rows: Query<&AttachCandidate>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut reattach: ResMut<ReattachState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    undo.checkpoint(&state, "Reattach Override");
    if state.doc.reattach_override(row.0, &row.1, row.2.clone()) {
        state.touch();
    }
    reattach.clear();
}

/// Keeps scene hotkeys off while a transform field is typing.
///
/// `keyboard_edit` reads key state directly rather than the focused input
/// stream, so Delete, Backspace and the arrows would otherwise nudge or delete
/// the selection under a half-typed value.
pub fn typing_guard(
    focus: Res<InputFocus>,
    fields: Query<&AxisValue>,
    node_fields: Query<&NodeAxisValue>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let Some(entity) = focus.get() else {
        return;
    };
    if !fields.contains(entity) && !node_fields.contains(entity) {
        return;
    }
    for key in [
        KeyCode::Delete,
        KeyCode::Backspace,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
    ] {
        keys.clear_just_pressed(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectKind;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    fn one_cube() -> (SceneState, u64, Undo) {
        let mut state = SceneState::default();
        let id = state.spawn(ObjectKind::Cube);
        state.select_only(id);
        (state, id, Undo::default())
    }

    #[test]
    fn titles_name_one_object_and_count_many() {
        assert_eq!(selection_title(0, None), "No selection");
        assert_eq!(selection_title(1, Some("Cube 1")), "Cube 1");
        assert_eq!(selection_title(1, None), "No selection");
        assert_eq!(selection_title(3, Some("Cube 1")), "3 Objects");
    }

    #[test]
    fn toggling_a_section_flips_only_that_section() {
        let mut sections = InspectorSections::default();
        for section in [
            Section::Transform,
            Section::Behaviour,
            Section::Visibility,
            Section::Script,
        ] {
            assert!(sections.is_open(section));
        }

        sections.toggle(Section::Visibility);
        assert!(!sections.is_open(Section::Visibility));
        assert!(sections.is_open(Section::Transform));
        assert!(sections.is_open(Section::Script));

        sections.toggle(Section::Visibility);
        assert!(sections.is_open(Section::Visibility));

        sections.set_open(Section::Script, false);
        assert!(!sections.is_open(Section::Script));
        assert!(sections.is_open(Section::Transform));
    }

    #[test]
    fn cycling_behaviour_writes_the_doc_with_one_checkpoint() {
        let (mut state, id, mut undo) = one_cube();
        assert_eq!(
            state.doc.get(id).unwrap().behaviour.kind,
            BehaviourKind::None
        );

        assert!(cycle_behaviour(&mut state, &mut undo));
        assert_eq!(
            state.doc.get(id).unwrap().behaviour.kind,
            BehaviourKind::Spin
        );
        assert!(cycle_behaviour(&mut state, &mut undo));
        assert_eq!(
            state.doc.get(id).unwrap().behaviour.kind,
            BehaviourKind::Bob
        );
        assert_eq!(undo.names().count(), 2);

        undo.undo(&mut state);
        assert_eq!(
            state.doc.get(id).unwrap().behaviour.kind,
            BehaviourKind::Spin
        );
    }

    #[test]
    fn behaviour_label_shows_the_speed_only_when_running() {
        assert_eq!(behaviour_label(Behaviour::default()), "None");
        let spinning = Behaviour {
            kind: BehaviourKind::Spin,
            speed: 2.5,
            amount: 0.25,
        };
        assert_eq!(behaviour_label(spinning), "Spin · 2.5");
        assert_eq!(
            behaviour_label(Behaviour {
                kind: BehaviourKind::Bob,
                ..spinning
            }),
            "Bob · 2.5"
        );
    }

    #[test]
    fn degrees_round_trip_through_a_quaternion() {
        let want = [12.5, -45.0, 30.0];
        let got = degrees(euler_quat(want));
        for (a, b) in got.iter().zip(want.iter()) {
            close(*a, *b);
        }
    }

    #[test]
    fn parsing_accepts_sign_decimals_and_exponents() {
        assert_eq!(parse_num(" -2.5 "), Some(-2.5));
        assert_eq!(parse_num("1e2"), Some(100.0));
        assert_eq!(parse_num(".5"), Some(0.5));
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("   "), None);
        assert_eq!(parse_num("nan"), None);
        assert_eq!(parse_num("zero"), None);
    }

    #[test]
    fn committing_position_writes_only_the_edited_axis() {
        let (mut state, id, mut undo) = one_cube();
        let before = state.doc.get(id).unwrap().translation;
        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Position, 1),
            "2.5"
        ));
        let got = state.doc.get(id).unwrap().translation;
        close(got[0], before[0]);
        close(got[1], 2.5);
        close(got[2], before[2]);
    }

    #[test]
    fn committing_rotation_stores_the_euler_inverse() {
        let (mut state, id, mut undo) = one_cube();
        state.doc.set_rotation(id, euler_quat([10.0, 20.0, 30.0]));
        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Rotation, 1),
            "45"
        ));
        let got = degrees(state.doc.get(id).unwrap().rotation);
        close(got[0], 10.0);
        close(got[1], 45.0);
        close(got[2], 30.0);
    }

    #[test]
    fn committing_scale_clamps_magnitude_and_preserves_sign() {
        let (mut state, id, mut undo) = one_cube();
        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Scale, 0),
            "0.001"
        ));
        close(state.doc.get(id).unwrap().scale[0], 0.01);

        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Scale, 1),
            "-0.001"
        ));
        close(state.doc.get(id).unwrap().scale[1], -0.01);

        assert!(!commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Scale, 2),
            "0"
        ));
        close(state.doc.get(id).unwrap().scale[2], 1.0);
    }

    #[test]
    fn bad_input_reverts_without_touching_the_doc_or_history() {
        let (mut state, id, mut undo) = one_cube();
        let want = state.doc.clone();
        for raw in ["", "   ", "abc", "1.2.3"] {
            assert!(!commit_field(
                &mut state,
                &mut undo,
                AxisValue(Prop::Position, 0),
                raw
            ));
        }
        assert_eq!(state.doc, want);
        assert!(!undo.can_undo());
        assert_eq!(
            state.doc.get(id).unwrap().translation,
            want.get(id).unwrap().translation
        );
    }

    #[test]
    fn one_checkpoint_per_commit_undoes_to_the_previous_value() {
        let (mut state, id, mut undo) = one_cube();
        let start = state.doc.get(id).unwrap().translation[0];
        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Position, 0),
            "4"
        ));
        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Position, 0),
            "5"
        ));
        undo.undo(&mut state);
        close(state.doc.get(id).unwrap().translation[0], 4.0);
        undo.undo(&mut state);
        close(state.doc.get(id).unwrap().translation[0], start);
    }

    #[test]
    fn editing_a_parent_position_moves_its_child() {
        let mut state = SceneState::default();
        let parent = state.spawn(ObjectKind::Empty);
        let child = state.spawn(ObjectKind::Cube);
        state.doc.set_translation(parent, [0.0; 3]);
        state.doc.set_translation(child, [2.0, 0.0, 0.0]);
        assert!(state.reparent(child, Some(parent)));
        state.select_only(parent);
        let mut undo = Undo::default();

        assert!(commit_field(
            &mut state,
            &mut undo,
            AxisValue(Prop::Position, 0),
            "3"
        ));
        close(state.doc.get(parent).unwrap().translation[0], 3.0);
        close(state.doc.get(child).unwrap().translation[0], 5.0);
    }
}

/// Host for the imported-model report above the object sections.
#[derive(Component, Clone, Copy, Default)]
pub struct ImportReportRows;

/// Marks the Reimport button and carries the file it re-imports.
#[derive(Component, Clone, Default)]
pub struct Reimport(pub PathBuf);

/// Marks the Options button and carries the file it opens options for.
#[derive(Component, Clone, Default)]
pub struct ImportOptions(pub PathBuf);

fn open_options(
    clicked: On<Activate>,
    buttons: Query<&ImportOptions>,
    mut panel: ResMut<crate::import::OptionsPanel>,
) {
    let Ok(button) = buttons.get(clicked.entity) else {
        return;
    };
    let folder = button
        .0
        .parent()
        .and_then(|parent| parent.strip_prefix(crate::build::workspace_dir()).ok())
        .map_or_else(|| PathBuf::from("assets"), Path::to_path_buf);
    let source = button.0.clone();
    panel.open_for(&source, &folder);
}

/// Renders the report of the model selected in the `FileSystem` dock.
pub fn sync_import_report(
    mut commands: Commands,
    tree: Res<crate::screens::editor::files::FileTree>,
    service: Res<crate::import::ImportService>,
    hosts: Query<Entity, With<ImportReportRows>>,
    mut shown: Local<Option<(Entity, String, String)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    let selected = tree.selected().unwrap_or_default().to_owned();
    let report = tree
        .selected()
        .map(|rel| tree.root_path().join(rel))
        .and_then(|path| service.report(&path).cloned());
    let signature = report
        .as_ref()
        .map(crate::import::ImportReport::summary)
        .unwrap_or_default();
    if shown.as_ref() == Some(&(host, selected.clone(), signature.clone())) {
        return;
    }
    *shown = Some((host, selected, signature));

    commands.entity(host).despawn_children();
    let Some(report) = report else {
        return;
    };

    let counts = format!(
        "{} nodes · {} meshes · {} verts · {} tris",
        report.counts.nodes, report.counts.meshes, report.counts.vertices, report.counts.triangles
    );
    let size = report.bounds.map_or_else(
        || "no geometry".to_string(),
        |bounds| {
            let size = bounds.size();
            format!("{:.2} × {:.2} × {:.2} m", size[0], size[1], size[2])
        },
    );
    let extras = format!(
        "{} textures · {} animations · {} warnings",
        report.textures.len(),
        report.animations.len(),
        report.warnings.len()
    );

    for line in [counts, size, extras] {
        let row = text(line, FONT_SIZE, tokens::TEXT);
        commands.spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
            }
            ChildOf(host)
            Children [ {row} ]
        });
    }

    let path = report.file.path.clone();
    let reimport_path = path.clone();
    let label = text("Reimport", FONT_SIZE, tokens::ACCENT);
    commands
        .spawn_scene(bsn! {
            Node {
                height: px(PROPERTY_ROW_H),
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(6)),
            }
            Button
            Reimport(reimport_path)
            ChildOf(host)
            Children [ {label} ]
        })
        .observe(reimport);

    let options_label = text("Import options…", FONT_SIZE, tokens::TEXT);
    commands
        .spawn_scene(bsn! {
            Node {
                height: px(PROPERTY_ROW_H),
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(6)),
            }
            Button
            ImportOptions(path)
            ChildOf(host)
            Children [ {options_label} ]
        })
        .observe(open_options);
}

fn reimport(
    activate: On<Activate>,
    buttons: Query<&Reimport>,
    mut service: ResMut<crate::import::ImportService>,
    mut log: ResMut<crate::console::ConsoleLog>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let settings = service.settings.clone();
    service.start(
        crate::import::ImportRequest {
            source: button.0.clone(),
            folder: PathBuf::new(),
            settings,
            copy: false,
        },
        &crate::build::workspace_dir(),
        &mut log,
    );
}
