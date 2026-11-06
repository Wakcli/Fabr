//! Scene-strip tabs: the authored scene and the running embedded game.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use eme::prelude::*;
use eme::scene::bsn;

use super::chrome::{self, Part, FONT_SIZE, SCENE_TAB_H};
use super::icons;
use crate::orbit::ViewMode;
use crate::tokens;

/// What a scene-strip tab points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind {
    /// The authored scene document.
    Scene,
    /// An embedded game session with this instance number.
    Instance(u32),
}

impl TabKind {
    /// The viewport mode this tab shows.
    fn view(self) -> ViewMode {
        match self {
            Self::Scene => ViewMode::Editor,
            Self::Instance(_) => ViewMode::Game,
        }
    }
}

/// One tab in the scene strip.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TabEntry {
    /// Text shown in the tab.
    pub label: String,
    /// What the tab shows.
    pub kind: TabKind,
}

/// The open scene-strip tabs and the selected one.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct Tabs {
    /// Tabs in display order.
    pub entries: Vec<TabEntry>,
    /// Index into [`Self::entries`] of the selected tab.
    pub active: usize,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            entries: vec![TabEntry {
                label: "main.ember".into(),
                kind: TabKind::Scene,
            }],
            active: 0,
        }
    }
}

impl Tabs {
    /// Index of the authored scene tab, if it is open.
    pub fn scene_index(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.kind == TabKind::Scene)
    }

    /// Renames the authored scene tab, as loading another scene from the file dock does.
    pub fn set_scene_label(&mut self, label: impl Into<String>) {
        if let Some(index) = self.scene_index() {
            self.entries[index].label = label.into();
        }
    }

    /// What the selected tab shows.
    pub fn active_kind(&self) -> TabKind {
        self.entries
            .get(self.active)
            .map_or(TabKind::Scene, |entry| entry.kind)
    }

    /// Selects `index` and mirrors its kind into `view`. Out-of-range indices are ignored.
    pub fn switch_to(&mut self, index: usize, view: &mut ViewMode) {
        if index >= self.entries.len() {
            return;
        }
        self.active = index;
        *view = self.active_kind().view();
    }

    /// Opens the embedded instance tab for `instance` and selects it, reusing an
    /// existing tab with the same number.
    pub fn open_instance(&mut self, instance: u32) {
        let kind = TabKind::Instance(instance);
        let index = match self.entries.iter().position(|entry| entry.kind == kind) {
            Some(index) => index,
            None => {
                self.entries.push(TabEntry {
                    label: format!("Instance {instance}#"),
                    kind,
                });
                self.entries.len() - 1
            }
        };
        self.active = index;
    }

    /// Removes every embedded instance tab and selects the authored scene.
    pub fn close_instances(&mut self) {
        self.entries
            .retain(|entry| !matches!(entry.kind, TabKind::Instance(_)));
        if let Some(index) = self.scene_index() {
            self.active = index;
        }
    }
}

/// Marks the strip section that holds the dynamic tab children.
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneTabs;

/// Marks a tab button with its index in [`Tabs::entries`].
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TabButton(usize);

/// The top strip: dynamic scene tabs, then the add and dock-menu controls.
pub(crate) fn strip() -> Part {
    Box::new(bsn! {
        Node {
            height: px(SCENE_TAB_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            padding: UiRect { right: px(6) },
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
            }
            SceneTabs
            --
            {chrome::icon_button(icons::add(), 24.0)}
            --
            {chrome::icon_button(icons::dock_menu(), 20.0)}
        ]
    })
}

fn tab_part(index: usize, entry: &TabEntry, active: bool) -> Part {
    let (bg, fg, line) = if active {
        (tokens::PANEL, tokens::TEXT, 2.0)
    } else {
        (tokens::WINDOW, tokens::TEXT_DIM, 0.0)
    };
    let icon = match entry.kind {
        TabKind::Scene => icons::badge(icons::SCENE),
        TabKind::Instance(_) => icons::mono(icons::INSTANCE, tokens::ICON),
    };
    let label = entry.label.clone();
    Box::new(bsn! {
        Node {
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(11)),
            border: UiRect { bottom: px(line) },
            column_gap: px(6),
        }
        ThemeBackgroundColor(bg)
        ThemeBorderColor(tokens::TAB_HIGHLIGHT)
        TabButton(index)
        Children [
            {icon}
            --
            {chrome::text(label, FONT_SIZE, fg)}
        ]
        on(tab_click)
    })
}

fn tab_click(
    mut click: On<PointerClick>,
    buttons: Query<&TabButton>,
    parents: Query<&ChildOf>,
    mut tabs: ResMut<Tabs>,
    mut view: ResMut<ViewMode>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.entity;
    loop {
        if let Ok(button) = buttons.get(entity) {
            click.propagate(false);
            tabs.switch_to(button.0, &mut view);
            return;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    }
}

/// Rebuild gate for the strip: one revision per distinct entry list and active tab.
#[derive(Default)]
pub(crate) struct TabStripSync {
    rev: u64,
    host: Option<Entity>,
    shown: Option<(Vec<TabEntry>, usize)>,
}

impl TabStripSync {
    /// Returns a new revision when `tabs` differs from the last accepted state.
    pub(crate) fn changed(&mut self, tabs: &Tabs) -> Option<u64> {
        let sig = (tabs.entries.clone(), tabs.active);
        if self.shown.as_ref() == Some(&sig) {
            return None;
        }
        self.shown = Some(sig);
        self.rev += 1;
        Some(self.rev)
    }
}

/// Rebuilds the strip children when tabs open, close, or the selection changes.
pub(crate) fn rebuild_strip(
    mut commands: Commands,
    hosts: Query<Entity, With<SceneTabs>>,
    tabs: Res<Tabs>,
    mut sync: Local<TabStripSync>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    if sync.host != Some(host) {
        sync.host = Some(host);
        sync.shown = None;
    }
    if sync.changed(&tabs).is_none() {
        return;
    }
    commands.entity(host).despawn_children();

    let kids: Vec<Part> = tabs
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| tab_part(index, entry, index == tabs.active))
        .collect();
    commands.spawn_scene(bsn! {
        Node {
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
        }
        ChildOf(host)
        Children [ {kids} ]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_a_scene_renames_the_authored_tab() {
        let mut tabs = Tabs::default();
        tabs.set_scene_label("cave.ember");
        assert_eq!(tabs.entries[0].label, "cave.ember");
        assert_eq!(tabs.active, 0);
    }

    #[test]
    fn the_default_strip_has_the_authored_scene_selected() {
        let tabs = Tabs::default();
        assert_eq!(tabs.entries.len(), 1);
        assert_eq!(tabs.entries[0].label, "main.ember");
        assert_eq!(tabs.active_kind(), TabKind::Scene);
        assert_eq!(tabs.active, 0);
    }

    #[test]
    fn opening_an_instance_reuses_its_existing_tab() {
        let mut tabs = Tabs::default();
        tabs.open_instance(1);
        let len = tabs.entries.len();
        tabs.open_instance(1);
        assert_eq!(tabs.entries.len(), len);
        assert_eq!(tabs.active, 1);
        assert_eq!(tabs.active_kind(), TabKind::Instance(1));
    }

    #[test]
    fn switching_tabs_maps_the_kind_onto_the_view_mode() {
        let mut tabs = Tabs::default();
        tabs.open_instance(1);
        let mut view = ViewMode::Editor;

        tabs.switch_to(1, &mut view);
        assert_eq!(view, ViewMode::Game);

        tabs.switch_to(0, &mut view);
        assert_eq!(view, ViewMode::Editor);
    }

    #[test]
    fn closing_instances_returns_to_the_scene_tab() {
        let mut tabs = Tabs::default();
        tabs.open_instance(1);
        tabs.close_instances();
        assert_eq!(tabs.entries.len(), 1);
        assert_eq!(tabs.active, 0);
        assert_eq!(tabs.active_kind(), TabKind::Scene);
    }

    #[test]
    fn the_strip_rebuilds_only_when_labels_or_the_active_tab_change() {
        let mut sync = TabStripSync::default();
        let mut tabs = Tabs::default();
        assert_eq!(sync.changed(&tabs), Some(1));
        assert_eq!(sync.changed(&tabs), None);

        tabs.entries[0].label = "other.scene".into();
        assert_eq!(sync.changed(&tabs), Some(2));

        tabs.open_instance(1);
        assert_eq!(sync.changed(&tabs), Some(3));

        let mut view = ViewMode::Game;
        tabs.switch_to(0, &mut view);
        assert_eq!(sync.changed(&tabs), Some(4));
    }
}
