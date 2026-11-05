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
use eme_game_scene::ObjectTransform;

use super::chrome::*;
use super::docks;
use super::icons;
use crate::scene::{self, SceneState};
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

/// An inspector section whose body can be collapsed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    Transform,
    Visibility,
    Script,
}

/// Which inspector sections are open. All sections start open.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct InspectorSections {
    pub transform: bool,
    pub visibility: bool,
    pub script: bool,
}

impl Default for InspectorSections {
    fn default() -> Self {
        Self {
            transform: true,
            visibility: true,
            script: true,
        }
    }
}

impl InspectorSections {
    /// Whether the body of `section` is visible.
    pub fn is_open(&self, section: Section) -> bool {
        match section {
            Section::Transform => self.transform,
            Section::Visibility => self.visibility,
            Section::Script => self.script,
        }
    }

    /// Shows or hides the body of `section`.
    pub fn set_open(&mut self, section: Section, open: bool) {
        match section {
            Section::Transform => self.transform = open,
            Section::Visibility => self.visibility = open,
            Section::Script => self.script = open,
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

pub fn right_dock() -> Part {
    let inspector = inspector_dock();
    let history = docks::history_dock();

    Box::new(bsn! {
        Node {
            width: px(RIGHT_DOCK_W),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            Node {
                flex_grow: 0.68,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {inspector} ]
            --
            {splitter(false)}
            --
            Node {
                flex_grow: 0.32,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {history} ]
        ]
    })
}

fn inspector_dock() -> Part {
    let tabs = vec![tab("Inspector", true)];
    let tail = vec![icon_button(icons::dock_menu(), 20.0)];

    let body = Box::new(bsn! {
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
            {object_row()}
            --
            {section(Section::Transform, "Transform", transform_rows())}
            --
            {section(Section::Visibility, "Visibility", vec![visibility_row()])}
            --
            {section(Section::Script, "Script", vec![script_row()])}
            --
            {spacer()}
        ]
    });

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            {tab_strip(tabs, tail, tokens::WINDOW)}
            --
            {body}
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
    mut titles: Query<&mut Text, With<ObjectTitle>>,
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
}

/// Keeps scene hotkeys off while a transform field is typing.
///
/// `keyboard_edit` reads key state directly rather than the focused input
/// stream, so Delete, Backspace and the arrows would otherwise nudge or delete
/// the selection under a half-typed value.
pub fn typing_guard(
    focus: Res<InputFocus>,
    fields: Query<&AxisValue>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let Some(entity) = focus.get() else {
        return;
    };
    if !fields.contains(entity) {
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
        for section in [Section::Transform, Section::Visibility, Section::Script] {
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
