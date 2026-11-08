//! Extension manager: enable or disable optional engine systems per project.

use std::path::PathBuf;

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button};
use eme_game_runtime::{Extension, ExtensionSet, PLUGINS_FILE};

use crate::screens::editor::chrome::{spacer, text, text_bold, Part, FONT_SIZE};
use crate::tokens;

/// Extension manager state.
#[derive(Resource, Default)]
pub struct ExtensionsState {
    pub open: bool,
    pub set: ExtensionSet,
    pub status: String,
    pub rev: u64,
    root: PathBuf,
}

impl ExtensionsState {
    pub fn load() -> Self {
        let root = crate::build::workspace_dir();
        let set = ExtensionSet::load(&root.join(PLUGINS_FILE));
        Self {
            open: false,
            set,
            status: String::new(),
            rev: 0,
            root,
        }
    }

    fn file(&self) -> PathBuf {
        self.root.join(PLUGINS_FILE)
    }

    /// Flips one extension and writes the project file.
    pub fn toggle(&mut self, extension: Extension) -> bool {
        let next = !self.set.enabled(extension);
        self.set.set(extension, next);
        match self.set.save(&self.file()) {
            Ok(()) => {
                self.status = format!(
                    "{} {} — restart to apply",
                    extension.label(),
                    if next { "enabled" } else { "disabled" }
                );
                self.rev = self.rev.wrapping_add(1);
                true
            }
            Err(err) => {
                self.status = err;
                self.rev = self.rev.wrapping_add(1);
                false
            }
        }
    }
}

/// Root of the extension window.
#[derive(Component, Clone, Copy, Default)]
pub struct ExtensionsRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct ExtensionsRows;

/// Toggle for one extension.
#[derive(Component, Clone, Copy, Default)]
pub struct ExtensionToggle(pub Extension);

/// Close button.
#[derive(Component, Clone, Copy, Default)]
pub struct CloseExtensions;

pub fn install(app: &mut App) {
    app.insert_resource(ExtensionsState::load())
        .add_systems(Update, sync);
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
        ExtensionsRows
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(560),
            top: px(110),
            width: px(430),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        ExtensionsRoot
        GlobalZIndex(338)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {host} ]
    })
}

fn sync(
    state: Res<ExtensionsState>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<ExtensionsRoot>>,
    hosts: Query<Entity, With<ExtensionsRows>>,
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

    let title = text_bold("Extensions", FONT_SIZE + 1.0, tokens::TEXT);
    let hint = text("project file fabr.plugins.ron", FONT_SIZE, tokens::TEXT_DIM);
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

    for extension in Extension::ALL {
        let on = state.set.enabled(extension);
        let label = text(extension.label(), FONT_SIZE, tokens::TEXT);
        let value = text(if on { "on" } else { "off" }, FONT_SIZE, tokens::ACCENT);
        commands
            .spawn_scene(bsn! {
                Node {
                    height: px(26),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                }
                Button
                ExtensionToggle(extension)
                ThemeBackgroundColor(tokens::CLEAR)
                ChildOf(host)
                Children [ {label} -- {spacer()} -- {value} ]
            })
            .observe(toggle);
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
            CloseExtensions
            ChildOf(actions)
            Children [ {close} ]
        })
        .observe(close_clicked);
}

fn toggle(
    activate: On<Activate>,
    toggles: Query<&ExtensionToggle>,
    mut state: ResMut<ExtensionsState>,
) {
    let Ok(toggle) = toggles.get(activate.entity) else {
        return;
    };
    state.toggle(toggle.0);
}

fn close_clicked(_: On<Activate>, mut state: ResMut<ExtensionsState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the window with the file's current state.
pub fn open(state: &mut ExtensionsState) {
    state.set = ExtensionSet::load(&state.file());
    state.status.clear();
    state.open = true;
    state.rev = state.rev.wrapping_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme-ext-ui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn toggling_writes_the_project_file() {
        let root = scratch("toggle");
        let mut state = ExtensionsState {
            open: true,
            set: ExtensionSet::default(),
            status: String::new(),
            rev: 0,
            root: root.clone(),
        };
        assert!(state.toggle(Extension::Scripting));
        let read = ExtensionSet::load(&root.join(PLUGINS_FILE));
        assert!(!read.enabled(Extension::Scripting));
        assert!(read.enabled(Extension::Physics));
        assert!(state.status.contains("restart"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
