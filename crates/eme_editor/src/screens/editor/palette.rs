//! Command palette: Ctrl+P opens a searchable list of editor commands.

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, InputFocus};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, ScrollArea, TextInput};

use super::chrome::{field, text, text_bold, FIELD_H, FONT_SIZE, Part};
use super::menu::{self, TopAction};
use super::icons;
use crate::tokens;

/// One command in the palette.
pub struct Command {
    /// Text shown in the list.
    pub label: &'static str,
    /// Action dispatched when the row runs.
    pub action: TopAction,
}

/// Every palette command, in display order.
pub const COMMANDS: &[Command] = &[
    Command {
        label: "Add Cube",
        action: TopAction::AddCube,
    },
    Command {
        label: "Add Empty",
        action: TopAction::AddEmpty,
    },
    Command {
        label: "Save Scene",
        action: TopAction::SaveScene,
    },
    Command {
        label: "Open Scene",
        action: TopAction::OpenScene,
    },
    Command {
        label: "Clear Console",
        action: TopAction::ClearConsole,
    },
    Command {
        label: "Reload Scripts",
        action: TopAction::ReloadScripts,
    },
    Command {
        label: "Project Settings",
        action: TopAction::ProjectSettings,
    },
    Command {
        label: "Build…",
        action: TopAction::Build,
    },
    Command {
        label: "Audio Mixer",
        action: TopAction::AudioMixer,
    },
    Command {
        label: "Package Manager",
        action: TopAction::PackageManager,
    },
    Command {
        label: "About Fabr",
        action: TopAction::About,
    },
];

/// Open state, query, keyboard selection, and the current row entities.
#[derive(Resource, Default)]
pub struct Palette {
    /// Whether the overlay is visible.
    pub open: bool,
    /// Text typed into the search field.
    pub query: String,
    /// Index into [`visible`](Self::visible).
    pub selected: usize,
    /// Bumped on every visible change.
    pub rev: u64,
    /// Row entities in filtered order, for Enter dispatch.
    pub rows: Vec<Entity>,
}

impl Palette {
    /// Commands matching the query, case-insensitively.
    pub fn visible(&self) -> Vec<&'static Command> {
        let query = self.query.trim().to_lowercase();
        COMMANDS
            .iter()
            .filter(|command| query.is_empty() || command.label.to_lowercase().contains(&query))
            .collect()
    }

    /// Moves the selection by `delta`, wrapping at both ends.
    pub fn move_selection(&mut self, delta: i32) {
        let len = self.visible().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected.min(len - 1) as i32;
        self.selected = (current + delta).rem_euclid(len as i32) as usize;
        self.rev = self.rev.wrapping_add(1);
    }

    /// Replaces the query and resets the selection.
    pub fn set_query(&mut self, query: String) {
        if query == self.query {
            return;
        }
        self.query = query;
        self.selected = 0;
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Root of the full-screen palette layer.
#[derive(Component, Clone, Copy, Default)]
pub struct PaletteRoot;

/// Host for the rebuilt command rows.
#[derive(Component, Clone, Copy, Default)]
pub struct PaletteRows;

/// The palette search input.
#[derive(Component, Clone, Copy, Default)]
pub struct PaletteInput;

/// Placeholder shown while the query is empty.
#[derive(Component, Clone, Copy, Default)]
pub struct PaletteHint;

/// The palette overlay, hidden until Ctrl+P.
pub fn overlay() -> Part {
    let input = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(20),
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new("")
        TextInput
        TabIndex(0)
        PaletteInput
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
    });
    let hint = Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(24),
            top: px(0),
            height: px(FIELD_H - 2.0),
            display: Display::Flex,
            align_items: AlignItems::Center,
        }
        PaletteHint
        Pickable::IGNORE
        Children [ {text("Type a command…", FONT_SIZE, tokens::TEXT_DIM)} ]
    });
    let search = Box::new(bsn! {
        Node { padding: UiRect::all(px(6)) }
        Button
        Children [ {field(vec![icons::search_dim(), input, hint], FIELD_H)} ]
        on(focus_input)
    });
    let rows = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            max_height: px(330),
            overflow: Overflow::scroll_y(),
        }
        ScrollArea
        PaletteRows
    }) as Part;
    let footer = text(
        "↑↓ navigate · Enter run · Esc close",
        FONT_SIZE,
        tokens::TEXT_DIM,
    );

    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect { top: px(90) },
        }
        PaletteRoot
        Button
        GlobalZIndex(340)
        ThemeBackgroundColor(tokens::SCRIM)
        on(dismiss)
        Children [
            Node {
                width: px(520),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                border: px(1),
                border_radius: px(5),
                overflow: Overflow::clip(),
            }
            ThemeBackgroundColor(tokens::WINDOW)
            ThemeBorderColor(tokens::BORDER)
            BoxShadow::new(Color::BLACK.with_alpha(0.65), px(0), px(0), px(3), px(16))
            Children [
                {search}
                --
                {rows}
                --
                Node {
                    padding: UiRect {
                        left: px(10),
                        right: px(10),
                        top: px(4),
                        bottom: px(6),
                    },
                }
                Children [ {footer} ]
            ]
        ]
    })
}

/// Closes the palette when the scrim is clicked.
fn dismiss(_: On<Activate>, mut palette: ResMut<Palette>, mut focus: ResMut<InputFocus>) {
    palette.open = false;
    palette.rev = palette.rev.wrapping_add(1);
    focus.clear();
}

/// Focuses the search field when its row is clicked.
fn focus_input(
    _: On<Activate>,
    inputs: Query<Entity, With<PaletteInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Ok(input) = inputs.single() {
        focus.set(input, FocusCause::Pressed);
    }
}

/// Closes the palette after a command runs.
fn close_on_activate(
    _: On<Activate>,
    mut palette: ResMut<Palette>,
    mut focus: ResMut<InputFocus>,
    inputs: Query<Entity, With<PaletteInput>>,
) {
    palette.open = false;
    palette.rev = palette.rev.wrapping_add(1);
    if inputs
        .single()
        .is_ok_and(|input| focus.get() == Some(input))
    {
        focus.clear();
    }
}

/// Ctrl+P toggles the palette; arrows move, Enter runs, Escape closes.
pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut palette: ResMut<Palette>,
    mut focus: ResMut<InputFocus>,
    mut inputs: Query<(Entity, &mut EditableText), With<PaletteInput>>,
    mut commands: Commands,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl && keys.just_pressed(KeyCode::KeyP) {
        let open = !palette.open;
        palette.open = open;
        palette.query.clear();
        palette.selected = 0;
        palette.rev = palette.rev.wrapping_add(1);
        if let Ok((entity, mut input)) = inputs.single_mut() {
            input.editor.set_text("");
            if open {
                focus.set(entity, FocusCause::Pressed);
            } else if focus.get() == Some(entity) {
                focus.clear();
            }
        }
        return;
    }
    if !palette.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        palette.open = false;
        palette.rev = palette.rev.wrapping_add(1);
        if let Ok((entity, _)) = inputs.single_mut()
            && focus.get() == Some(entity)
        {
            focus.clear();
        }
        return;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        palette.move_selection(1);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        palette.move_selection(-1);
    }
    if keys.just_pressed(KeyCode::Enter)
        && let Some(entity) = palette.rows.get(palette.selected).copied()
    {
        commands.trigger(Activate { entity });
    }
}

/// Shows the overlay and rebuilds rows when the query or selection changes.
pub fn sync(
    mut palette: ResMut<Palette>,
    mut commands: Commands,
    mut nodes: Query<
        (&mut Node, Has<PaletteRoot>),
        Or<(With<PaletteRoot>, With<PaletteHint>)>,
    >,
    hosts: Query<Entity, With<PaletteRows>>,
    inputs: Query<&EditableText, With<PaletteInput>>,
    mut shown: Local<Option<(Entity, u64, bool)>>,
) {
    if palette.open
        && let Ok(input) = inputs.single()
    {
        palette.set_query(input.value().to_string());
    }

    for (mut node, is_root) in &mut nodes {
        let want = if is_root {
            if palette.open {
                Display::Flex
            } else {
                Display::None
            }
        } else if palette.query.is_empty() {
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
    let signature = (host, palette.rev, palette.open);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    super::chrome::despawn_children(&mut commands, host);
    palette.rows.clear();
    if !palette.open {
        return;
    }

    let visible = palette.visible();
    let selected = palette.selected.min(visible.len().saturating_sub(1));
    palette.selected = selected;
    for (index, command) in visible.iter().enumerate() {
        let active = index == selected;
        let (bg, fg) = if active {
            (tokens::SELECT, tokens::SELECT_TEXT)
        } else {
            (tokens::CLEAR, tokens::TEXT)
        };
        let label = text_bold(command.label, FONT_SIZE, fg);
        let entity = commands
            .spawn_scene(bsn! {
                Node {
                    height: px(26),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(10)),
                    column_gap: px(6),
                }
                Button
                ThemeBackgroundColor(bg)
                ChildOf(host)
                Children [ {label} ]
            })
            .insert(command.action)
            .observe(menu::run_top_action)
            .observe(close_on_activate)
            .id();
        palette.rows.push(entity);
    }
    if visible.is_empty() {
        let line = text("No matching commands", FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node {
                height: px(26),
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
            }
            ChildOf(host)
            Children [ {line} ]
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::BuildState;
    use crate::console::ConsoleLog;
    use crate::scene::{ModelIndex, SceneState};
    use crate::settings::ProjectSettingsState;
    use crate::undo::Undo;
    use eme_game_scene::ObjectKind;

    #[test]
    fn filtering_is_case_insensitive() {
        let palette = Palette {
            query: "CUBE".into(),
            ..Default::default()
        };
        let labels: Vec<&str> = palette.visible().iter().map(|command| command.label).collect();
        assert_eq!(labels, ["Add Cube"]);

        let palette = Palette {
            query: "scene".into(),
            ..Default::default()
        };
        let labels: Vec<&str> = palette.visible().iter().map(|command| command.label).collect();
        assert_eq!(labels, ["Save Scene", "Open Scene"]);
    }

    #[test]
    fn an_empty_query_lists_every_command() {
        let palette = Palette::default();
        assert_eq!(palette.visible().len(), COMMANDS.len());
        assert!(!palette.visible().is_empty());
    }

    fn selected_action(palette: &Palette) -> Option<TopAction> {
        palette
            .visible()
            .get(palette.selected)
            .map(|command| command.action)
    }

    #[test]
    fn selection_wraps_and_maps_to_an_action() {
        let mut palette = Palette {
            query: "scene".into(),
            ..Default::default()
        };
        assert_eq!(selected_action(&palette), Some(TopAction::SaveScene));
        palette.move_selection(1);
        assert_eq!(selected_action(&palette), Some(TopAction::OpenScene));
        palette.move_selection(1);
        assert_eq!(selected_action(&palette), Some(TopAction::SaveScene));
        palette.move_selection(-1);
        assert_eq!(selected_action(&palette), Some(TopAction::OpenScene));
    }

    fn action_app() -> App {
        let mut app = App::new();
        app.insert_resource(menu::TopMenu::default())
            .insert_resource(SceneState::default())
            .insert_resource(Undo::default())
            .insert_resource(ConsoleLog::default())
            .insert_resource(eme_script::ScriptReload::default())
            .insert_resource(ModelIndex::default())
            .insert_resource(BuildState::default())
            .insert_resource(crate::mixer::MixerState::load())
            .insert_resource(crate::packages::PackagesState::load())
            .insert_resource(ProjectSettingsState {
                open: false,
                settings: Default::default(),
                error: String::new(),
                status: String::new(),
                rev: 0,
                root: std::env::temp_dir(),
            })
            .add_observer(menu::run_top_action);
        app
    }

    #[test]
    fn a_palette_row_runs_its_command() {
        let mut app = action_app();
        let entity = app.world_mut().spawn(TopAction::AddCube).id();
        app.world_mut().trigger(Activate { entity });
        let state = app.world().resource::<SceneState>();
        assert_eq!(state.doc.len(), 1);
        assert_eq!(state.doc.objects[0].kind, ObjectKind::Cube);
    }

    #[test]
    fn the_build_command_opens_the_build_dialog() {
        let mut app = action_app();
        let entity = app.world_mut().spawn(TopAction::Build).id();
        app.world_mut().trigger(Activate { entity });
        assert!(app.world().resource::<BuildState>().open);
    }
}
