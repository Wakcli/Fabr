//! Named dock layout presets stored under `layouts/`.

use std::path::PathBuf;

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, InputFocus};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, TextInput};
use eme_editor_core::Layout;

use crate::screens::editor::chrome::{field, spacer, text, text_bold, Part, FIELD_H, FONT_SIZE};
use crate::tokens;
use crate::LayoutRes;

/// Folder that stores layout presets, relative to the project root.
pub const LAYOUTS_DIR: &str = "layouts";

/// Whether `name` can be used as a preset file name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Layout presets on disk.
#[derive(Clone, Debug)]
pub struct LayoutPresets {
    dir: PathBuf,
}

impl Default for LayoutPresets {
    fn default() -> Self {
        Self::at(crate::build::workspace_dir().join(LAYOUTS_DIR))
    }
}

impl LayoutPresets {
    /// Presets stored under `dir`.
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Preset names, sorted.
    pub fn list(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()? != "ron" {
                    return None;
                }
                Some(path.file_stem()?.to_string_lossy().into_owned())
            })
            .filter(|name| valid_name(name))
            .collect();
        names.sort();
        names
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        valid_name(name).then(|| self.dir.join(format!("{name}.ron")))
    }

    /// Writes `layout` under `name`, validating both.
    pub fn save(&self, name: &str, layout: &Layout) -> Result<(), String> {
        let path = self
            .file(name)
            .ok_or_else(|| format!("{name:?} is not a valid preset name"))?;
        layout.validate()?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|err| format!("{}: {err}", self.dir.display()))?;
        crate::write_layout(layout, &path)
    }

    /// Reads a preset, rejecting missing and invalid files.
    pub fn load(&self, name: &str) -> Result<Layout, String> {
        let path = self
            .file(name)
            .ok_or_else(|| format!("{name:?} is not a valid preset name"))?;
        crate::read_layout(&path).ok_or_else(|| format!("{} is missing or invalid", path.display()))
    }

    /// Removes a preset.
    pub fn delete(&self, name: &str) -> Result<(), String> {
        let path = self
            .file(name)
            .ok_or_else(|| format!("{name:?} is not a valid preset name"))?;
        std::fs::remove_file(&path).map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// Layout preset window state.
#[derive(Resource, Default)]
pub struct LayoutsState {
    /// Whether the window is visible.
    pub open: bool,
    /// Name draft, seeded when the window opens.
    pub name: String,
    /// Last success message.
    pub status: String,
    /// Last error message.
    pub error: String,
    /// Bumped to rebuild the list.
    pub rev: u64,
    /// Presets on disk.
    pub presets: LayoutPresets,
}

impl LayoutsState {
    /// Builds state from the project folder.
    pub fn load() -> Self {
        Self::default()
    }

    /// Clears the messages before the window is shown.
    pub fn reset(&mut self) {
        self.name.clear();
        self.status.clear();
        self.error.clear();
        self.open = true;
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Root of the floating window.
#[derive(Component, Clone, Copy, Default)]
pub struct LayoutsRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct LayoutsRows;

/// Preset name field.
#[derive(Component, Clone, Copy, Default)]
pub struct LayoutNameInput;

/// Save button.
#[derive(Component, Clone, Copy, Default)]
pub struct SavePreset;

/// Load button for one preset.
#[derive(Component, Clone, Default)]
pub struct LoadPreset(pub String);

/// Delete button for one preset.
#[derive(Component, Clone, Default)]
pub struct DeletePreset(pub String);

/// Close button.
#[derive(Component, Clone, Copy, Default)]
pub struct CloseLayouts;

pub fn install(app: &mut App) {
    app.insert_resource(LayoutsState::load())
        .add_systems(Update, sync);
}

pub fn panel() -> Part {
    let host = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(3),
            padding: px(6),
        }
        LayoutsRows
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(540),
            top: px(130),
            width: px(460),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        LayoutsRoot
        GlobalZIndex(341)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {host} ]
    })
}

fn sync(
    state: Res<LayoutsState>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<LayoutsRoot>>,
    hosts: Query<Entity, With<LayoutsRows>>,
    mut shown: Local<Option<(Entity, u64, bool, String)>>,
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
    let list = state.presets.list().join(",");
    let key = (host, state.rev, state.open, list);
    if shown.as_ref() == Some(&key) {
        return;
    }
    *shown = Some(key);
    commands.entity(host).despawn_children();
    if !state.open {
        return;
    }

    let title = text_bold("Layout Presets", FONT_SIZE + 1.0, tokens::TEXT);
    let hint = text("saved under layouts/", FONT_SIZE, tokens::TEXT_DIM);
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            height: px(26),
            padding: UiRect::horizontal(px(4)),
        }
        ChildOf(host)
        Children [ {title} -- {hint} -- {spacer()} ]
    });

    let draft = state.name.clone();
    let name_input = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(20),
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(draft)
        TextInput
        TabIndex(0)
        LayoutNameInput
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        on(focus_name)
    });
    let save_label = text("Save current", FONT_SIZE, tokens::TEXT);
    let save = commands
        .spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            SavePreset
            Children [ {save_label} ]
        })
        .observe(save_current)
        .id();
    let row = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
                height: px(30),
                padding: UiRect::horizontal(px(4)),
            }
            ChildOf(host)
            Children [ {field(vec![name_input], FIELD_H)} ]
        })
        .id();
    commands.entity(row).add_child(save);

    if state.presets.list().is_empty() {
        let line = text("No saved presets", FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(4)), display: Display::Flex, align_items: AlignItems::Center }
            ChildOf(host)
            Children [ {line} ]
        });
    }
    for name in state.presets.list() {
        let label = text(name.clone(), FONT_SIZE, tokens::TEXT);
        let load_text = text("Load", FONT_SIZE, tokens::TEXT);
        let delete_text = text("Delete", FONT_SIZE, tokens::TEXT_DIM);
        let load_name = name.clone();
        let delete_name = name.clone();
        let row = commands
            .spawn_scene(bsn! {
                Node {
                    height: px(26),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    padding: UiRect::horizontal(px(6)),
                }
                ChildOf(host)
                Children [ {label} -- {spacer()} ]
            })
            .id();
        commands
            .spawn_scene(bsn! {
                Node { height: px(22), padding: UiRect::horizontal(px(8)), display: Display::Flex, align_items: AlignItems::Center }
                Button
                LoadPreset(load_name)
                ChildOf(row)
                Children [ {load_text} ]
            })
            .observe(load_preset);
        commands
            .spawn_scene(bsn! {
                Node { height: px(22), padding: UiRect::horizontal(px(8)), display: Display::Flex, align_items: AlignItems::Center }
                Button
                DeletePreset(delete_name)
                ChildOf(row)
                Children [ {delete_text} ]
            })
            .observe(delete_preset);
    }

    if !state.error.is_empty() {
        let error = text(state.error.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(4)) }
            ChildOf(host)
            Children [ {error} ]
        });
    }
    if !state.status.is_empty() {
        let status = text(state.status.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(4)) }
            ChildOf(host)
            Children [ {status} ]
        });
    }

    let close_text = text("Close", FONT_SIZE, tokens::TEXT);
    let actions = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexEnd,
                padding: UiRect::all(px(4)),
                height: px(30),
            }
            ChildOf(host)
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            CloseLayouts
            ChildOf(actions)
            Children [ {close_text} ]
        })
        .observe(close);
}

fn focus_name(
    press: On<PointerPress>,
    inputs: Query<(), With<LayoutNameInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !inputs.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

fn save_current(
    _: On<Activate>,
    mut fields: Query<&mut EditableText, With<LayoutNameInput>>,
    mut state: ResMut<LayoutsState>,
    layout: Res<LayoutRes>,
) {
    let Ok(mut field) = fields.single_mut() else {
        return;
    };
    let name = field.value().to_string();
    let name = name.trim();
    state.status.clear();
    state.error.clear();
    match state.presets.save(name, &layout) {
        Ok(()) => {
            state.status = format!("saved {name}");
            state.name.clear();
            field.editor.set_text("");
        }
        Err(err) => state.error = err,
    }
    state.rev = state.rev.wrapping_add(1);
}

fn load_preset(
    activate: On<Activate>,
    buttons: Query<&LoadPreset>,
    mut state: ResMut<LayoutsState>,
    mut layout: ResMut<LayoutRes>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    state.status.clear();
    state.error.clear();
    match state.presets.load(&button.0) {
        Ok(preset) => {
            layout.0 = preset;
            crate::screens::editor::dock::save_layout(&layout);
            state.status = format!("loaded {}", button.0);
        }
        Err(err) => state.error = err,
    }
    state.rev = state.rev.wrapping_add(1);
}

fn delete_preset(
    activate: On<Activate>,
    buttons: Query<&DeletePreset>,
    mut state: ResMut<LayoutsState>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    state.status.clear();
    state.error.clear();
    match state.presets.delete(&button.0) {
        Ok(()) => state.status = format!("deleted {}", button.0),
        Err(err) => state.error = err,
    }
    state.rev = state.rev.wrapping_add(1);
}

fn close(_: On<Activate>, mut state: ResMut<LayoutsState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the window with a clean draft.
pub fn open(state: &mut LayoutsState) {
    state.reset();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme-layouts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_reject_paths_and_odd_characters() {
        assert!(valid_name("wide"));
        assert!(valid_name("wide-2_x"));
        assert!(!valid_name(""));
        assert!(!valid_name("../escape"));
        assert!(!valid_name("with space"));
        assert!(!valid_name("x".repeat(40).as_str()));
    }

    #[test]
    fn presets_round_trip_on_disk() {
        let dir = scratch("roundtrip");
        let presets = LayoutPresets::at(dir.clone());
        let mut layout = Layout::default_editor();
        assert!(layout.set_ratio_at(1, 0.45));
        presets.save("wide", &layout).unwrap();
        assert_eq!(presets.list(), ["wide"]);
        assert_eq!(presets.load("wide").unwrap(), layout);
        presets.delete("wide").unwrap();
        assert!(presets.list().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_files_and_bad_names_are_rejected() {
        let dir = scratch("broken");
        let presets = LayoutPresets::at(dir.clone());
        std::fs::write(dir.join("bad.ron"), "not a layout").unwrap();
        assert!(presets.load("bad").is_err());
        assert!(presets.load("missing").is_err());
        assert!(presets.save("../escape", &Layout::default_editor()).is_err());
        assert!(presets.delete("nope").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
