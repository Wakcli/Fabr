//! Customisable editor keyboard shortcuts, stored in `fabr.shortcuts.ron`.

use alloc::collections::BTreeMap;
use std::path::PathBuf;

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::input_focus::tab_navigation::TabIndex;
use eme::input::keyboard::KeyCode;
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, TextInput};
use serde::{Deserialize, Serialize};

use crate::screens::editor::chrome::{spacer, text, text_bold, Part, FONT_SIZE};
use crate::screens::editor::palette;
use crate::tokens;

/// File holding the project's shortcut bindings.
pub const SHORTCUTS_FILE: &str = "fabr.shortcuts.ron";

/// One bindable editor command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Command {
    /// Open or close the command palette.
    #[default]
    Palette,
    /// Save the active scene.
    SaveScene,
    /// Reload the active scene from disk.
    OpenScene,
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
    /// Delete the selection.
    Delete,
    /// Duplicate the selection.
    Duplicate,
    /// Start or stop the embedded game.
    Play,
}

impl Command {
    /// Every command, in display order.
    pub const ALL: [Command; 8] = [
        Command::Palette,
        Command::SaveScene,
        Command::OpenScene,
        Command::Undo,
        Command::Redo,
        Command::Delete,
        Command::Duplicate,
        Command::Play,
    ];

    /// Stable identifier used in the project file.
    pub fn id(self) -> &'static str {
        match self {
            Command::Palette => "command_palette",
            Command::SaveScene => "save_scene",
            Command::OpenScene => "open_scene",
            Command::Undo => "undo",
            Command::Redo => "redo",
            Command::Delete => "delete",
            Command::Duplicate => "duplicate",
            Command::Play => "play",
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            Command::Palette => "Command palette",
            Command::SaveScene => "Save scene",
            Command::OpenScene => "Open scene",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Delete => "Delete selection",
            Command::Duplicate => "Duplicate selection",
            Command::Play => "Play / stop",
        }
    }

    /// Command for a stored identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
}

/// One key chord with modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chord {
    /// Ctrl held.
    pub ctrl: bool,
    /// Shift held.
    pub shift: bool,
    /// Alt held.
    pub alt: bool,
    /// Key name, such as `P`, `F5` or `S`.
    pub key: String,
}

impl Chord {
    /// Parses `"Ctrl+Shift+P"`, `"F5"` or `"Ctrl+S"`.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.trim().ends_with('+') {
            return Err("missing key after '+'".into());
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut parts: Vec<&str> = text.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return Err("bind a key, for example Ctrl+S".into());
        }
        let key = parts.pop().unwrap().to_ascii_uppercase();
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" => alt = true,
                other => return Err(format!("unknown modifier {other:?}")),
            }
        }
        if key.is_empty() {
            return Err("missing key".into());
        }
        Ok(Self {
            ctrl,
            shift,
            alt,
            key,
        })
    }

    /// Canonical display text.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(match self.key.as_str() {
            "DELETE" => "Delete",
            "ESCAPE" => "Esc",
            "SPACE" => "Space",
            "ENTER" => "Enter",
            other => other,
        });
        out
    }

    /// Matching key code, or `None` for an unsupported key name.
    pub fn key_code(&self) -> Option<KeyCode> {
        match self.key.as_str() {
            "DELETE" => return Some(KeyCode::Delete),
            "ESCAPE" | "ESC" => return Some(KeyCode::Escape),
            "SPACE" => return Some(KeyCode::Space),
            "ENTER" => return Some(KeyCode::Enter),
            _ => {}
        }
        if let Some(rest) = self.key.strip_prefix('F')
            && let Ok(number) = rest.parse::<u8>()
            && (1..=12).contains(&number)
        {
            return Some(
                [
                    KeyCode::F1,
                    KeyCode::F2,
                    KeyCode::F3,
                    KeyCode::F4,
                    KeyCode::F5,
                    KeyCode::F6,
                    KeyCode::F7,
                    KeyCode::F8,
                    KeyCode::F9,
                    KeyCode::F10,
                    KeyCode::F11,
                    KeyCode::F12,
                ][(number - 1) as usize],
            );
        }
        let mut chars = self.key.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return None;
        };
        ch.to_ascii_uppercase()
            .is_ascii_uppercase()
            .then(|| letter_code(ch.to_ascii_uppercase()))
            .flatten()
    }

    /// Whether the current keyboard state matches this chord.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>) -> bool {
        let Some(key) = self.key_code() else {
            return false;
        };
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        ctrl == self.ctrl && shift == self.shift && alt == self.alt && keys.just_pressed(key)
    }
}

fn letter_code(ch: char) -> Option<KeyCode> {
    Some(match ch {
        'A' => KeyCode::KeyA,
        'B' => KeyCode::KeyB,
        'C' => KeyCode::KeyC,
        'D' => KeyCode::KeyD,
        'E' => KeyCode::KeyE,
        'F' => KeyCode::KeyF,
        'G' => KeyCode::KeyG,
        'H' => KeyCode::KeyH,
        'I' => KeyCode::KeyI,
        'J' => KeyCode::KeyJ,
        'K' => KeyCode::KeyK,
        'L' => KeyCode::KeyL,
        'M' => KeyCode::KeyM,
        'N' => KeyCode::KeyN,
        'O' => KeyCode::KeyO,
        'P' => KeyCode::KeyP,
        'Q' => KeyCode::KeyQ,
        'R' => KeyCode::KeyR,
        'S' => KeyCode::KeyS,
        'T' => KeyCode::KeyT,
        'U' => KeyCode::KeyU,
        'V' => KeyCode::KeyV,
        'W' => KeyCode::KeyW,
        'X' => KeyCode::KeyX,
        'Y' => KeyCode::KeyY,
        'Z' => KeyCode::KeyZ,
        _ => return None,
    })
}

/// The project's shortcut table.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Shortcuts {
    /// Command identifier to chord text.
    pub binds: BTreeMap<String, String>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        let mut binds = BTreeMap::new();
        binds.insert(Command::Palette.id().into(), "Ctrl+P".into());
        binds.insert(Command::SaveScene.id().into(), "Ctrl+S".into());
        binds.insert(Command::OpenScene.id().into(), "Ctrl+O".into());
        binds.insert(Command::Undo.id().into(), "Ctrl+Z".into());
        binds.insert(Command::Redo.id().into(), "Ctrl+Shift+Z".into());
        binds.insert(Command::Delete.id().into(), "Delete".into());
        binds.insert(Command::Duplicate.id().into(), "Ctrl+D".into());
        binds.insert(Command::Play.id().into(), "F5".into());
        Self { binds }
    }
}

impl Shortcuts {
    /// Reads the project file, falling back to the defaults.
    pub fn load(path: &std::path::Path) -> Self {
        let Ok(src) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match ron::from_str::<Self>(&src) {
            Ok(set) if set.validate().is_ok() => set,
            Ok(set) => {
                warn!(
                    "{} is invalid ({}); using the default shortcuts",
                    path.display(),
                    set.validate().err().unwrap_or_default()
                );
                Self::default()
            }
            Err(err) => {
                warn!("{} is malformed ({err}); using the default shortcuts", path.display());
                Self::default()
            }
        }
    }

    /// Writes the file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Parsed chord for `command`, falling back to the built-in default.
    pub fn chord(&self, command: Command) -> Chord {
        self.binds
            .get(command.id())
            .and_then(|text| Chord::parse(text).ok())
            .or_else(|| {
                Shortcuts::default()
                    .binds
                    .get(command.id())
                    .and_then(|text| Chord::parse(text).ok())
            })
            .expect("default shortcuts parse")
    }

    /// Replaces one binding, validating it and rejecting duplicates.
    pub fn bind(&mut self, command: Command, text: &str) -> Result<Chord, String> {
        let chord = Chord::parse(text)?;
        if chord.key_code().is_none() {
            return Err(format!("{:?} is not a key this editor binds", chord.key));
        }
        if let Some((other, _)) = self
            .binds
            .iter()
            .filter(|(id, _)| id.as_str() != command.id())
            .find_map(|(id, text)| {
                Chord::parse(text)
                    .ok()
                    .filter(|known| *known == chord)
                    .map(|known| (id.clone(), known))
            })
        {
            return Err(format!(
                "{} is already bound to {}",
                chord.label(),
                Command::from_id(&other).map_or("another command", |cmd| cmd.label())
            ));
        }
        self.binds.insert(command.id().into(), chord.label());
        Ok(chord)
    }

    /// Checks every entry parses and no two commands share a chord.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen: Vec<(Chord, &str)> = Vec::new();
        for (id, text) in &self.binds {
            let Some(command) = Command::from_id(id) else {
                return Err(format!("unknown command {id:?}"));
            };
            let chord = Chord::parse(text)?;
            if chord.key_code().is_none() {
                return Err(format!("{}: unsupported key in {text:?}", command.label()));
            }
            if let Some((_, other)) = seen.iter().find(|(known, _)| *known == chord) {
                return Err(format!("{id} duplicates the chord of {other}"));
            }
            seen.push((chord, id));
        }
        Ok(())
    }

    /// Command whose chord fires for the current keyboard state.
    pub fn fire(&self, keys: &ButtonInput<KeyCode>) -> Option<Command> {
        Command::ALL
            .into_iter()
            .find(|command| self.chord(*command).pressed(keys))
    }
}

/// Live shortcut table.
#[derive(Resource, Clone, Debug, Default)]
pub struct ShortcutConfig(pub Shortcuts);

/// Editor state of the shortcuts window.
#[derive(Resource, Default)]
pub struct ShortcutsState {
    pub open: bool,
    /// Draft chord per command.
    pub drafts: Vec<(Command, String)>,
    pub status: String,
    pub error: String,
    pub rev: u64,
    root: PathBuf,
}

impl ShortcutsState {
    pub fn load() -> Self {
        let root = crate::build::workspace_dir();
        let set = Shortcuts::load(&root.join(SHORTCUTS_FILE));
        Self {
            open: false,
            drafts: Command::ALL
                .into_iter()
                .map(|command| (command, set.chord(command).label()))
                .collect(),
            status: String::new(),
            error: String::new(),
            rev: 0,
            root,
        }
    }

    fn file(&self) -> PathBuf {
        self.root.join(SHORTCUTS_FILE)
    }

    /// Applies a draft chord to `command` and writes the file.
    pub fn apply(&mut self, config: &mut ShortcutConfig, command: Command, text: &str) -> bool {
        self.error.clear();
        self.status.clear();
        match config.0.bind(command, text) {
            Ok(chord) => {
                if let Some(slot) = self.drafts.iter_mut().find(|(cmd, _)| *cmd == command) {
                    slot.1 = chord.label();
                }
                match config.0.save(&self.file()) {
                    Ok(()) => {
                        self.status = format!("{} is {}", command.label(), chord.label());
                        self.rev = self.rev.wrapping_add(1);
                        true
                    }
                    Err(err) => {
                        self.error = err;
                        self.rev = self.rev.wrapping_add(1);
                        false
                    }
                }
            }
            Err(err) => {
                self.error = err;
                self.rev = self.rev.wrapping_add(1);
                false
            }
        }
    }
}

/// Root of the shortcuts window.
#[derive(Component, Clone, Copy, Default)]
pub struct ShortcutsRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct ShortcutsRows;

/// Editable chord field.
#[derive(Component, Clone, Copy, Default)]
pub struct ShortcutField(pub Command);

/// Save button.
#[derive(Component, Clone, Copy, Default)]
pub struct SaveShortcuts;

/// Close button.
#[derive(Component, Clone, Copy, Default)]
pub struct CloseShortcuts;

pub fn install(app: &mut App) {
    let root = crate::build::workspace_dir();
    let set = Shortcuts::load(&root.join(SHORTCUTS_FILE));
    app.insert_resource(ShortcutConfig(set))
        .insert_resource(ShortcutsState::load())
        .add_systems(Update, (sync, keys));
}

pub fn panel() -> Part {
    let host = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(3),
            padding: px(4),
        }
        ShortcutsRows
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(560),
            top: px(120),
            width: px(480),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        ShortcutsRoot
        GlobalZIndex(337)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {host} ]
    })
}

fn sync(
    state: Res<ShortcutsState>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<ShortcutsRoot>>,
    hosts: Query<Entity, With<ShortcutsRows>>,
    mut shown: Local<Option<(Entity, u64, bool)>>,
) {
    for mut root in &mut roots {
        let want = if state.open {
            Display::Flex
        } else {
            Display::None
        };
        if root.display != want {
            root.display = want;
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

    let title = text_bold("Keyboard Shortcuts", FONT_SIZE + 1.0, tokens::TEXT);
    let hint = text("type a chord, then Enter", FONT_SIZE, tokens::TEXT_DIM);
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            height: px(30),
            padding: UiRect::horizontal(px(8)),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        ChildOf(host)
        Children [ {title} -- {hint} -- {spacer()} ]
    });

    for (command, draft) in &state.drafts {
        let label = text(command.label(), FONT_SIZE, tokens::TEXT);
        let value = draft.clone();
        let seat = *command;
        let field = Box::new(bsn! {
            Node {
                width: px(150),
                height: px(22),
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(6)),
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
            ShortcutField(seat)
            on(focus_field)
        }) as Part;
        commands.spawn_scene(bsn! {
            Node {
                height: px(26),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(host)
            Children [ {label} -- {spacer()} -- {field} ]
        });
    }

    if !state.error.is_empty() {
        let error = text(state.error.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)) }
            ChildOf(host)
            Children [ {error} ]
        });
    }
    if !state.status.is_empty() {
        let status = text(state.status.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)) }
            ChildOf(host)
            Children [ {status} ]
        });
    }

    let close = text("Close", FONT_SIZE, tokens::TEXT);
    let actions = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(4),
                padding: UiRect::all(px(6)),
                height: px(34),
            }
            ChildOf(host)
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            SaveShortcuts
            ChildOf(actions)
            Children [ {text("Save", FONT_SIZE, tokens::TEXT)} ]
        })
        .observe(save_clicked);
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            CloseShortcuts
            ChildOf(actions)
            Children [ {close} ]
        })
        .observe(close_clicked);
}

fn focus_field(
    press: On<PointerPress>,
    fields: Query<(), With<ShortcutField>>,
    mut focus: ResMut<eme::input_focus::InputFocus>,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    focus.set(press.entity, eme::input_focus::FocusCause::Pressed);
}

fn save_clicked(
    _: On<Activate>,
    fields: Query<(&ShortcutField, &EditableText)>,
    mut state: ResMut<ShortcutsState>,
    mut config: ResMut<ShortcutConfig>,
) {
    for (field, value) in &fields {
        let text = value.value().to_string();
        state.apply(&mut config, field.0, &text);
    }
}

fn close_clicked(_: On<Activate>, mut state: ResMut<ShortcutsState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the window with drafts from the current bindings.
pub fn open(state: &mut ShortcutsState, config: &ShortcutConfig) {
    state.drafts = Command::ALL
        .into_iter()
        .map(|command| (command, config.0.chord(command).label()))
        .collect();
    state.open = true;
    state.status.clear();
    state.error.clear();
    state.rev = state.rev.wrapping_add(1);
}

/// Dispatches the configured shortcuts.
pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<ShortcutConfig>,
    mut palette: ResMut<palette::Palette>,
    mut focus: ResMut<eme::input_focus::InputFocus>,
    mut inputs: Query<(Entity, &mut EditableText), With<palette::PaletteInput>>,
    texts: Query<(), With<TextInput>>,
    mut state: ResMut<crate::scene::SceneState>,
    mut undo: ResMut<crate::undo::Undo>,
    mut play: ResMut<crate::play::PlayState>,
    mut view: ResMut<crate::orbit::ViewMode>,
    mut tabs: ResMut<crate::screens::editor::tabs::Tabs>,
    index: Res<crate::scene::ModelIndex>,
    mut log: ResMut<crate::console::ConsoleLog>,
) {
    if focus.get().is_some_and(|entity| texts.contains(entity)) {
        return;
    }
    let Some(command) = config.0.fire(&keys) else {
        return;
    };
    match command {
        Command::Palette => palette.toggle(&mut focus, &mut inputs),
        Command::SaveScene => match crate::persist::save_to(&state.doc, crate::persist::SCENE_PATH) {
            Ok(path) => log.push(format!("saved scene to {}", path.display())),
            Err(err) => log.push(format!("could not save scene: {err}")),
        },
        Command::OpenScene => {
            crate::persist::open_path_into(
                &mut state,
                crate::persist::SCENE_PATH,
                &index,
                &mut log,
            );
        }
        Command::Undo => {
            if undo.can_undo() {
                undo.undo(&mut state);
                log.push("undo".to_string());
            }
        }
        Command::Redo => {
            if undo.can_redo() {
                undo.redo(&mut state);
                log.push("redo".to_string());
            }
        }
        Command::Delete => {
            if state.selection.is_empty() {
                return;
            }
            undo.checkpoint(&state, "Delete");
            let removed = state.despawn_selected();
            log.push(format!("deleted {removed} object(s)"));
        }
        Command::Duplicate => {
            let count = crate::clipboard::duplicate_selection(&mut state, &mut undo);
            log.push(format!("duplicated {count} object(s)"));
        }
        Command::Play => {
            if play.running() {
                play.stop_embedded(&mut state, &mut view, &mut tabs);
                play.stop();
                log.push("stopped the game".to_string());
            } else {
                play.start_embedded(&mut state, &mut view, &mut tabs);
                log.push("started the game".to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_and_format() {
        let chord = Chord::parse("ctrl+shift+p").unwrap();
        assert!(chord.ctrl && chord.shift && !chord.alt);
        assert_eq!(chord.key, "P");
        assert_eq!(chord.label(), "Ctrl+Shift+P");
        assert_eq!(Chord::parse("F5").unwrap().key_code(), Some(KeyCode::F5));
        assert!(Chord::parse("Ctrl+").is_err());
        assert!(Chord::parse("Meta+P").is_err());
        assert!(Chord::parse("Ctrl+P+Q").is_err());
    }

    #[test]
    fn defaults_are_valid_and_unique() {
        let set = Shortcuts::default();
        set.validate().unwrap();
        assert_eq!(set.chord(Command::SaveScene).label(), "Ctrl+S");
        assert_eq!(set.chord(Command::Palette).label(), "Ctrl+P");
        assert_eq!(set.chord(Command::Undo).label(), "Ctrl+Z");
        assert_eq!(set.chord(Command::Delete).label(), "Delete");
        assert_eq!(set.chord(Command::Play).label(), "F5");
    }

    #[test]
    fn binding_rejects_duplicates_and_unknown_keys() {
        let mut set = Shortcuts::default();
        assert!(set.bind(Command::SaveScene, "Ctrl+Q").is_ok());
        assert!(set.bind(Command::OpenScene, "Ctrl+Q").is_err());
        assert!(set.bind(Command::OpenScene, "Ctrl+Nope").is_err());
        assert_eq!(set.chord(Command::SaveScene).label(), "Ctrl+Q");
    }

    #[test]
    fn the_file_round_trips_and_ignores_broken_input() {
        let dir = std::env::temp_dir().join(format!("eme-shortcuts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SHORTCUTS_FILE);
        let mut set = Shortcuts::default();
        set.bind(Command::Palette, "Ctrl+K").unwrap();
        set.save(&path).unwrap();
        assert_eq!(Shortcuts::load(&path), set);

        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(Shortcuts::load(&path), Shortcuts::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn applying_a_draft_writes_the_project_file() {
        let dir = std::env::temp_dir().join(format!("eme-shortcuts-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = ShortcutsState {
            open: true,
            drafts: Vec::new(),
            status: String::new(),
            error: String::new(),
            rev: 0,
            root: dir.clone(),
        };
        let mut config = ShortcutConfig::default();
        assert!(state.apply(&mut config, Command::Palette, "Ctrl+K"));
        let saved = Shortcuts::load(&dir.join(SHORTCUTS_FILE));
        assert_eq!(saved.chord(Command::Palette).label(), "Ctrl+K");
        assert!(!state.apply(&mut config, Command::OpenScene, "Ctrl+K"));
        assert!(!state.error.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fire_maps_the_pressed_chord_to_its_command() {
        let set = Shortcuts::default();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyP);
        assert_eq!(set.fire(&keys), Some(Command::Palette));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyS);
        assert_eq!(set.fire(&keys), Some(Command::SaveScene));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyD);
        assert_eq!(set.fire(&keys), Some(Command::Duplicate));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::Delete);
        assert_eq!(set.fire(&keys), Some(Command::Delete));

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyS);
        assert_eq!(set.fire(&keys), None);
    }
}
