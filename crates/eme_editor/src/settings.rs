//! Project Settings window: player and input settings stored beside the project.

use std::path::PathBuf;

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
use eme::ui_widgets::{Activate, Button, ScrollArea, TextInput};
use eme_editor_project::ProjectSettings;

use crate::console::ConsoleLog;
use crate::screens::editor::chrome::{self, spacer, Part, FONT_SIZE};
use crate::tokens;

/// Which draft field a text row edits.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum FieldKind {
    /// Window title.
    #[default]
    Title,
    /// Window width.
    Width,
    /// Window height.
    Height,
    /// MSAA sample count.
    Msaa,
    /// One input action, keys comma-separated.
    Action(String),
}

/// Open state and draft settings.
#[derive(Resource)]
pub struct ProjectSettingsState {
    /// Whether the window is visible.
    pub open: bool,
    /// Draft values being edited.
    pub settings: ProjectSettings,
    /// File or validation error.
    pub error: String,
    /// Last save result.
    pub status: String,
    /// Bumped to rebuild the body.
    pub rev: u64,
    /// Project root containing the settings file.
    pub root: PathBuf,
}

impl ProjectSettingsState {
    fn load() -> Self {
        let root = crate::build::workspace_dir();
        let path = root.join(eme_editor_project::SETTINGS_FILE);
        let (settings, error) = match ProjectSettings::load_or_default(&path) {
            Ok(settings) => (settings, String::new()),
            Err(err) => (ProjectSettings::default(), err),
        };
        Self {
            open: false,
            settings,
            error,
            status: String::new(),
            rev: 0,
            root,
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(eme_editor_project::SETTINGS_FILE)
    }
}

/// Installs the settings resource, input actions and UI systems.
pub fn install(app: &mut App) {
    let state = ProjectSettingsState::load();
    let actions = eme_script::InputActions::from_names(&state.settings.input.actions)
        .unwrap_or_default();
    app.insert_resource(actions)
        .insert_resource(state)
        .add_systems(Update, sync);
}

/// Root of the floating settings window.
#[derive(Component, Clone, Copy, Default)]
pub struct SettingsRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct SettingsRows;

/// One editable settings field.
#[derive(Component, Clone, Default)]
pub struct SettingsField(pub FieldKind);

/// Focused settings field.
#[derive(Component, Clone, Copy, Default)]
pub struct SettingsFieldEdit;

/// The vsync toggle.
#[derive(Component, Clone, Copy, Default)]
pub struct VsyncToggle;

/// Save button.
#[derive(Component, Clone, Copy, Default)]
pub struct SaveSettings;

/// Close button.
#[derive(Component, Clone, Copy, Default)]
pub struct CloseSettings;

/// The floating settings window.
pub fn panel() -> Part {
    let body = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            max_height: px(620),
            overflow: Overflow::scroll_y(),
        }
        ScrollArea
        Children [ {rows_host()} ]
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(520),
            top: px(100),
            width: px(520),
            max_height: percent(80),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(4),
            padding: px(2),
            overflow: Overflow::clip(),
        }
        SettingsRoot
        GlobalZIndex(330)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {body} ]
    })
}

fn rows_host() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        SettingsRows
    })
}

/// Shows or hides the window and rebuilds its body.
pub fn sync(
    state: Res<ProjectSettingsState>,
    mut roots: Query<&mut Node, With<SettingsRoot>>,
    hosts: Query<Entity, With<SettingsRows>>,
    mut commands: Commands,
    mut shown: Local<Option<(Entity, u64, bool)>>,
) {
    for mut node in &mut roots {
        let want = if state.open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let signature = (host, state.rev, state.open);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    commands.entity(host).despawn_children();
    if !state.open {
        return;
    }

    let header = text("Project Settings", FONT_SIZE, tokens::TEXT);
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            height: px(28),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        ChildOf(host)
        Children [ {header} -- {spacer()} ]
    });

    section(host, &mut commands, "Player");
    field_row(host, &mut commands, "Title", &state.settings.player.title, FieldKind::Title);
    field_row(
        host,
        &mut commands,
        "Width",
        &state.settings.player.width.to_string(),
        FieldKind::Width,
    );
    field_row(
        host,
        &mut commands,
        "Height",
        &state.settings.player.height.to_string(),
        FieldKind::Height,
    );
    toggle_row(host, &mut commands, "VSync", state.settings.player.vsync);

    section(host, &mut commands, "Graphics");
    field_row(
        host,
        &mut commands,
        "MSAA",
        &state.settings.graphics.msaa.to_string(),
        FieldKind::Msaa,
    );

    section(host, &mut commands, "Input Actions");
    for (action, keys) in &state.settings.input.actions {
        field_row(
            host,
            &mut commands,
            action,
            &keys.join(", "),
            FieldKind::Action(action.clone()),
        );
    }

    if !state.error.is_empty() {
        let error = text(state.error.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)), display: Display::Flex }
            ChildOf(host)
            Children [ {error} ]
        });
    }
    if !state.status.is_empty() {
        let status = text(state.status.clone(), FONT_SIZE, tokens::ACCENT);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)), display: Display::Flex }
            ChildOf(host)
            Children [ {status} ]
        });
    }

    let save = text("Save", FONT_SIZE, tokens::TEXT);
    let close = text("Close", FONT_SIZE, tokens::TEXT);
    let actions = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(4),
                padding: UiRect::horizontal(px(8)),
                height: px(32),
            }
            ChildOf(host)
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            SaveSettings
            ChildOf(actions)
            Children [ {save} ]
        })
        .observe(save_settings);
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            CloseSettings
            ChildOf(actions)
            Children [ {close} ]
        })
        .observe(close_settings);
}

fn section(host: Entity, commands: &mut Commands, title: &str) {
    let title = text(title.to_string(), FONT_SIZE, tokens::SECTION_TEXT);
    commands.spawn_scene(bsn! {
        Node { padding: UiRect::horizontal(px(8)), height: px(22), display: Display::Flex, align_items: AlignItems::Center }
        ThemeBackgroundColor(tokens::CATEGORY)
        ChildOf(host)
        Children [ {title} ]
    });
}

fn field_row(host: Entity, commands: &mut Commands, label: &str, value: &str, kind: FieldKind) {
    let label = text(label.to_string(), FONT_SIZE, tokens::TEXT);
    let value = value.to_owned();
    let row = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                column_gap: px(6),
                height: px(24),
            }
            ChildOf(host)
            Children [ {label} -- {spacer()} ]
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node {
                width: px(220),
                height: px(20),
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(4)),
                overflow: Overflow::clip(),
            }
            ThemeBackgroundColor(tokens::FIELD)
            EditableText::new(value)
            TextInput
            TabIndex(0)
            TextLayout { linebreak: LineBreak::NoWrap }
            TextFont {
                font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
                font_size: FontSize::Px(FONT_SIZE),
                weight: FontWeight::NORMAL,
            }
            ThemeTextColor(tokens::TEXT)
            ChildOf(row)
        })
        .insert(SettingsField(kind))
        .observe(focus_field)
        .observe(blur_field)
        .observe(field_key);
}

fn toggle_row(host: Entity, commands: &mut Commands, label: &str, value: bool) {
    let label = text(label.to_string(), FONT_SIZE, tokens::TEXT);
    let value = text(
        if value { "on" } else { "off" },
        FONT_SIZE,
        tokens::ACCENT,
    );
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                height: px(24),
            }
            Button
            VsyncToggle
            ChildOf(host)
            Children [ {label} -- {spacer()} -- {value} ]
        })
        .observe(toggle_vsync);
}

fn text(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    chrome::text(value, size, token)
}

fn focus_field(
    press: On<PointerPress>,
    fields: Query<&SettingsField>,
    mut values: Query<&mut EditableText>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    commands.entity(press.entity).insert(SettingsFieldEdit);
    if let Ok(mut value) = values.get_mut(press.entity) {
        value.queue_edit(TextEdit::SelectAll);
    }
    focus.set(press.entity, FocusCause::Pressed);
}

fn field_key(
    key: On<FocusedInput<KeyboardInput>>,
    fields: Query<(&SettingsField, &EditableText)>,
    mut state: ResMut<ProjectSettingsState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let input = &key.input;
    if !input.state.is_pressed() || input.repeat {
        return;
    }
    let entity = key.focused_entity;
    if input.key_code == KeyCode::Escape {
        commands.entity(entity).remove::<SettingsFieldEdit>();
        return;
    }
    if input.key_code != KeyCode::Enter {
        return;
    }
    let Ok((field, value)) = fields.get(entity) else {
        return;
    };
    commit(&mut state, &field.0, value.value().to_string().as_str());
    commands.entity(entity).remove::<SettingsFieldEdit>();
    focus.clear();
}

fn blur_field(
    lost: On<FocusLost>,
    edits: Query<&SettingsFieldEdit>,
    fields: Query<(&SettingsField, &EditableText)>,
    mut state: ResMut<ProjectSettingsState>,
    mut commands: Commands,
) {
    let entity = lost.event_target();
    if !edits.contains(entity) {
        return;
    }
    commands.entity(entity).remove::<SettingsFieldEdit>();
    let Ok((field, value)) = fields.get(entity) else {
        return;
    };
    commit(&mut state, &field.0, value.value().to_string().as_str());
}

fn commit(state: &mut ProjectSettingsState, kind: &FieldKind, raw: &str) {
    state.status.clear();
    state.error.clear();
    match kind {
        FieldKind::Title => state.settings.player.title = raw.to_owned(),
        FieldKind::Width => match raw.trim().parse::<u32>() {
            Ok(value) if value > 0 => state.settings.player.width = value,
            _ => state.error = "width must be a positive number".into(),
        },
        FieldKind::Height => match raw.trim().parse::<u32>() {
            Ok(value) if value > 0 => state.settings.player.height = value,
            _ => state.error = "height must be a positive number".into(),
        },
        FieldKind::Msaa => match raw.trim().parse::<u32>() {
            Ok(value @ (1 | 2 | 4 | 8)) => state.settings.graphics.msaa = value,
            _ => state.error = "MSAA must be 1, 2, 4 or 8".into(),
        },
        FieldKind::Action(action) => {
            let keys: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            state.settings.input.actions.insert(action.clone(), keys);
        }
    }
}

fn toggle_vsync(_: On<Activate>, mut state: ResMut<ProjectSettingsState>) {
    state.settings.player.vsync = !state.settings.player.vsync;
    state.rev = state.rev.wrapping_add(1);
}

fn save_settings(
    _: On<Activate>,
    mut state: ResMut<ProjectSettingsState>,
    mut actions: ResMut<eme_script::InputActions>,
    mut log: ResMut<ConsoleLog>,
) {
    let path = state.path();
    match state.settings.save(&path) {
        Ok(()) => {
            match eme_script::InputActions::from_names(&state.settings.input.actions) {
                Ok(next) => {
                    *actions = next;
                    state.status = format!("saved {}", path.display());
                    state.error.clear();
                }
                Err(err) => {
                    state.error = err;
                    state.status.clear();
                }
            }
            log.push(format!("saved project settings to {}", path.display()));
        }
        Err(err) => {
            state.error = err.clone();
            state.status.clear();
            log.push(format!("could not save project settings: {err}"));
        }
    }
    state.rev = state.rev.wrapping_add(1);
}

fn close_settings(_: On<Activate>, mut state: ResMut<ProjectSettingsState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the window from a menu or shortcut.
pub fn open(state: &mut ProjectSettingsState) {
    state.open = true;
    state.rev = state.rev.wrapping_add(1);
}

