//! Package Manager: install local packages and stage their assets into builds.

use std::path::PathBuf;

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, InputFocus};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, TextInput};
use eme_editor_project::{PackageSet, PACKAGES_FILE};

use crate::console::ConsoleLog;
use crate::screens::editor::chrome::{
    field, spacer, text, text_bold, Part, FIELD_H, FONT_SIZE,
};
use crate::screens::editor::icons;
use crate::tokens;

/// Package Manager state and the installed list.
#[derive(Resource, Default)]
pub struct PackagesState {
    /// Whether the window is visible.
    pub open: bool,
    /// Installed packages.
    pub set: PackageSet,
    /// Folder typed into the add row.
    pub draft: String,
    /// Last success message.
    pub status: String,
    /// Last error message.
    pub error: String,
    /// Bumped to rebuild the rows.
    pub rev: u64,
    root: PathBuf,
}

impl PackagesState {
    /// Builds state from the current project file.
    pub fn load() -> Self {
        let root = crate::build::workspace_dir();
        let (set, error) = match PackageSet::load(&root.join(PACKAGES_FILE)) {
            Ok(set) => (set, String::new()),
            Err(err) => (PackageSet::default(), err),
        };
        Self {
            open: false,
            set,
            draft: String::new(),
            status: String::new(),
            error,
            rev: 0,
            root,
        }
    }

    fn file(&self) -> PathBuf {
        self.root.join(PACKAGES_FILE)
    }

    /// Adds the package in `dir`, returning whether it was installed.
    pub fn add(&mut self, dir: &str) -> bool {
        self.status.clear();
        self.error.clear();
        match self.set.add(&self.root, dir) {
            Ok(manifest) => {
                self.status = format!("installed {} {}", manifest.name, manifest.version);
                self.draft.clear();
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

    /// Removes the package at `index`.
    pub fn remove(&mut self, index: usize) -> bool {
        self.status.clear();
        self.error.clear();
        let removed = self.set.remove(index);
        if removed {
            self.status = "package removed; save to write the list".into();
            self.rev = self.rev.wrapping_add(1);
        }
        removed
    }

    /// Writes `fabr.packages.ron`.
    pub fn save(&mut self) -> bool {
        let path = self.file();
        match self.set.save(&path) {
            Ok(()) => {
                self.status = format!("saved {}", path.display());
                self.error.clear();
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
}

/// Installs the resource, window and the packages staged into builds.
pub fn install(app: &mut App) {
    app.insert_resource(PackagesState::load())
        .add_systems(Update, (sync, sync_hint));
}

/// Root of the floating window.
#[derive(Component, Clone, Copy, Default)]
pub struct PackagesRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct PackagesRows;

/// Remove button for one installed package.
#[derive(Component, Clone, Copy, Default)]
pub struct RemovePackage(pub usize);

/// Add button.
#[derive(Component, Clone, Copy, Default)]
pub struct AddPackage;

/// Save button.
#[derive(Component, Clone, Copy, Default)]
pub struct SavePackages;

/// Close button.
#[derive(Component, Clone, Copy, Default)]
pub struct ClosePackages;

/// The package folder text input.
#[derive(Component, Clone, Copy, Default)]
pub struct PackagePathInput;

/// Placeholder for the folder input.
#[derive(Component, Clone, Copy, Default)]
pub struct PackagePathHint;

pub fn panel() -> Part {
    let host = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(3),
            padding: px(4),
        }
        PackagesRows
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(540),
            top: px(110),
            width: px(540),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        PackagesRoot
        GlobalZIndex(336)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {host} ]
    })
}

pub fn sync(
    state: Res<PackagesState>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<PackagesRoot>>,
    hosts: Query<Entity, With<PackagesRows>>,
    mut shown: Local<Option<(Entity, u64, bool, u64)>>,
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
    let signature = (host, state.rev, state.open, state.root.capacity() as u64);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    commands.entity(host).despawn_children();
    if !state.open {
        return;
    }

    let title = text_bold("Package Manager", FONT_SIZE + 1.0, tokens::TEXT);
    let hint = text(
        "local packages with a fabr.package.ron manifest",
        FONT_SIZE,
        tokens::TEXT_DIM,
    );
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

    if state.set.packages.is_empty() {
        let empty = text("No packages installed", FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(8)), align_items: AlignItems::Center, display: Display::Flex }
            ChildOf(host)
            Children [ {empty} ]
        });
    }

    for (index, entry) in state.set.packages.iter().enumerate() {
        let manifest = PackageSet::manifest(&state.root, entry);
        let (name, version) = match &manifest {
            Ok(manifest) => (manifest.name.clone(), manifest.version.clone()),
            Err(_) => (entry.path.clone(), "invalid".into()),
        };
        let label = text_bold(&name, FONT_SIZE, tokens::TEXT);
        let version = text(version, FONT_SIZE, tokens::TEXT_DIM);
        let path = text(entry.path.clone(), FONT_SIZE, tokens::TEXT_DIM);
        let remove = text("Remove", FONT_SIZE, tokens::TEXT);
        let row = commands
            .spawn_scene(bsn! {
                Node {
                    height: px(26),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    padding: UiRect::horizontal(px(8)),
                }
                ChildOf(host)
                Children [ {label} -- {version} -- {path} -- {spacer()} ]
            })
            .id();
        commands
            .spawn_scene(bsn! {
                Node { height: px(22), padding: UiRect::horizontal(px(8)), display: Display::Flex, align_items: AlignItems::Center }
                Button
                RemovePackage(index)
                ChildOf(row)
                Children [ {remove} ]
            })
            .observe(remove_package);
        if let Err(err) = manifest {
            let error = text(err, FONT_SIZE, tokens::TEXT_DIM);
            commands.spawn_scene(bsn! {
                Node { padding: UiRect::horizontal(px(8)) }
                ChildOf(host)
                Children [ {error} ]
            });
        }
    }

    let draft = state.draft.clone();
    let input = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(20),
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(draft)
        TextInput
        TabIndex(0)
        PackagePathInput
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        on(focus_path)
    });
    let placeholder = Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(24),
            top: px(0),
            height: px(FIELD_H - 2.0),
            display: Display::Flex,
            align_items: AlignItems::Center,
        }
        PackagePathHint
        Pickable::IGNORE
        Children [ {text("packages/my-pack", FONT_SIZE, tokens::TEXT_DIM)} ]
    });
    let add_label = text("Add", FONT_SIZE, tokens::TEXT);
    let add = commands
        .spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            AddPackage
            Children [ {add_label} ]
        })
        .observe(add_package)
        .id();
    let field = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect::horizontal(px(8)),
            }
            Button
            ThemeBackgroundColor(tokens::CLEAR)
            ChildOf(host)
            Children [ {field(vec![icons::search_dim(), input, placeholder], FIELD_H)} ]
            on(focus_path_row)
        })
        .id();
    commands.entity(field).add_child(add);

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

    let save = text("Save", FONT_SIZE, tokens::TEXT);
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
            SavePackages
            ChildOf(actions)
            Children [ {save} ]
        })
        .observe(save_packages);
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            ClosePackages
            ChildOf(actions)
            Children [ {close} ]
        })
        .observe(close_packages);
}

/// Hides the placeholder while the folder field has text.
pub fn sync_hint(
    inputs: Query<&EditableText, With<PackagePathInput>>,
    mut hints: Query<&mut Node, With<PackagePathHint>>,
) {
    let filled = inputs
        .single()
        .is_ok_and(|input| !input.value().to_string().is_empty());
    let want = if filled { Display::None } else { Display::Flex };
    for mut node in &mut hints {
        if node.display != want {
            node.display = want;
        }
    }
}

fn focus_path(
    press: On<PointerPress>,
    inputs: Query<(), With<PackagePathInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !inputs.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

fn focus_path_row(
    _: On<Activate>,
    inputs: Query<Entity, With<PackagePathInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if let Ok(input) = inputs.single() {
        focus.set(input, FocusCause::Pressed);
    }
}

fn add_package(
    _: On<Activate>,
    inputs: Query<&EditableText, With<PackagePathInput>>,
    mut state: ResMut<PackagesState>,
    mut log: ResMut<ConsoleLog>,
) {
    let dir = inputs
        .single()
        .map(|input| input.value().to_string())
        .unwrap_or_default();
    let added = state.add(&dir);
    let line = if added {
        state.status.clone()
    } else {
        format!("package: {}", state.error)
    };
    log.push(line);
}

fn remove_package(
    activate: On<Activate>,
    rows: Query<&RemovePackage>,
    mut state: ResMut<PackagesState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.remove(row.0);
}

fn save_packages(
    _: On<Activate>,
    mut state: ResMut<PackagesState>,
    mut log: ResMut<ConsoleLog>,
) {
    if state.save() {
        log.push(format!("saved {}", state.file().display()));
    } else {
        log.push(format!("package list: {}", state.error));
    }
}

fn close_packages(_: On<Activate>, mut state: ResMut<PackagesState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the window, reloading the list from disk.
pub fn open(state: &mut PackagesState) {
    match PackageSet::load(&state.file()) {
        Ok(set) => {
            state.set = set;
            state.error.clear();
        }
        Err(err) => state.error = err,
    }
    state.open = true;
    state.status.clear();
    state.rev = state.rev.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("eme-packages-ui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("packages/sky/assets")).unwrap();
        std::fs::write(
            dir.join("packages/sky/fabr.package.ron"),
            r#"(name: "sky", version: "1.0.0", assets: "assets")"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn adding_and_saving_writes_the_project_list() {
        let root = scratch("add");
        let mut state = PackagesState {
            open: true,
            set: PackageSet::default(),
            draft: String::new(),
            status: String::new(),
            error: String::new(),
            rev: 0,
            root: root.clone(),
        };
        assert!(state.add("packages/sky"));
        assert_eq!(state.set.packages.len(), 1);
        assert!(state.save());
        let text = std::fs::read_to_string(root.join(PACKAGES_FILE)).unwrap();
        assert!(text.contains("packages/sky"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_invalid_folder_reports_an_error() {
        let root = scratch("bad");
        let mut state = PackagesState {
            open: true,
            set: PackageSet::default(),
            draft: String::new(),
            status: String::new(),
            error: String::new(),
            rev: 0,
            root: root.clone(),
        };
        assert!(!state.add("packages/missing"));
        assert!(!state.error.is_empty());
        assert!(state.set.packages.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
