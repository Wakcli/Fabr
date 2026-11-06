//! Docking frame built from the editor [`Layout`].
//!
//! Every leaf owns one [`Dock`] with the panel id the layout assigned, so a later
//! drag and drop pass can move tabs between docks without re-deriving identity
//! from node position. [`DockRoot`] marks the node whose children are replaced
//! when the layout changes.

use eme::feathers::theme::ThemeBackgroundColor;
use eme::prelude::*;
use eme::scene::bsn;
use eme_editor_core::{Axis, Layout, PanelId, PanelKind};

use super::chrome::{self, splitter, tab, tab_strip, Part, STATUS_H};
use super::{docks, files, icons, inspector, viewport};
use crate::tokens;
use crate::LayoutRes;

/// Panel name understood as the fixed chrome strip rather than a resizable dock.
const STATUS: &str = "Status";

/// Panel name that renders the undo history content.
const HISTORY: &str = "History";

/// Host of the dock tree; its children are rebuilt from [`Layout`].
#[derive(Component, Clone, Copy, Default)]
pub struct DockRoot;

/// Identity of one leaf panel and the drop target for a tab.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dock {
    /// Panel this dock renders.
    pub id: PanelId,
}

impl Default for Dock {
    fn default() -> Self {
        Self { id: PanelId(0) }
    }
}

/// The tab strip title for `kind`.
pub fn title(kind: &PanelKind) -> String {
    match kind {
        PanelKind::Hierarchy => "Scene".into(),
        PanelKind::Scene => "Viewport".into(),
        PanelKind::Inspector => "Inspector".into(),
        PanelKind::Project => "FileSystem".into(),
        PanelKind::Console => "Console".into(),
        PanelKind::Custom(name) => name.clone(),
    }
}

/// The full dock frame for `layout`, including its [`DockRoot`] host.
pub fn tree(layout: &Layout) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        DockRoot
        Children [ {content(layout)} ]
    })
}

/// Rebuilds the dock children when the [`Layout`] resource changes.
pub fn sync(mut commands: Commands, roots: Query<Entity, With<DockRoot>>, layout: Res<LayoutRes>) {
    if !layout.is_changed() {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };
    commands.entity(root).despawn_children();
    commands.spawn_scene(bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        ChildOf(root)
        Children [ {content(&layout)} ]
    });
}

fn content(layout: &Layout) -> Part {
    match layout {
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => split(*axis, *ratio, first, second),
        Layout::Panel { id, kind } => dock(*id, kind),
    }
}

fn split(axis: Axis, ratio: f32, first: &Layout, second: &Layout) -> Part {
    let (dir, vertical) = match axis {
        Axis::Horizontal => (FlexDirection::Row, true),
        Axis::Vertical => (FlexDirection::Column, false),
    };

    let (a, b) = match (fixed_extent(first), fixed_extent(second)) {
        (Some(h), _) => (
            sized(content(first), Sizing::Fixed(h)),
            sized(content(second), Sizing::Grow),
        ),
        (None, Some(h)) => (
            sized(content(first), Sizing::Grow),
            sized(content(second), Sizing::Fixed(h)),
        ),
        _ => (
            sized(content(first), Sizing::Ratio(ratio * 100.0)),
            sized(content(second), Sizing::Grow),
        ),
    };

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: {dir},
            align_items: AlignItems::Stretch,
        }
        Children [ {a} -- {splitter(vertical)} -- {b} ]
    })
}

// the status strip is fixed chrome, so its panel ignores the layout ratio
fn fixed_extent(layout: &Layout) -> Option<f32> {
    match layout {
        Layout::Panel {
            kind: PanelKind::Custom(name),
            ..
        } if name == STATUS => Some(STATUS_H),
        _ => None,
    }
}

/// How a split gives its child its share of the main axis.
enum Sizing {
    /// Percentage of the axis, fixed at the layout ratio.
    Ratio(f32),
    /// Fixed logical pixels, for chrome that must not stretch.
    Fixed(f32),
    /// Whatever the sibling leaves over.
    Grow,
}

fn sized(child: Part, sizing: Sizing) -> Part {
    let (grow, flex_basis, shrink): (f32, Val, f32) = match sizing {
        Sizing::Ratio(share) => (0.0, percent(share), 0.0),
        Sizing::Fixed(extent) => (0.0, px(extent), 0.0),
        Sizing::Grow => (1.0, px(0), 1.0),
    };
    Box::new(bsn! {
        Node {
            flex_grow: {grow},
            flex_basis: {flex_basis},
            flex_shrink: {shrink},
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        Children [ {child} ]
    })
}

fn dock(id: PanelId, kind: &PanelKind) -> Part {
    if matches!(kind, PanelKind::Custom(name) if name == STATUS) {
        return Box::new(bsn! {
            Node {
                flex_grow: 1.0,
                min_width: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::clip(),
            }
            Dock { id }
            Children [ {super::status_bar()} ]
        });
    }

    let label = title(kind);
    let strip = tab_strip(vec![tab(&label, true)], tail(kind), tokens::WINDOW);

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        Dock { id }
        Children [
            {strip}
            --
            {body(kind)}
        ]
    })
}

fn tail(kind: &PanelKind) -> Vec<Part> {
    let menu = chrome::icon_button(icons::dock_menu(), 20.0);
    match kind {
        PanelKind::Project => vec![files::refresh_button(), menu],
        _ => vec![menu],
    }
}

fn body(kind: &PanelKind) -> Part {
    match kind {
        PanelKind::Hierarchy => docks::hierarchy(),
        PanelKind::Scene => viewport::body(),
        PanelKind::Inspector => inspector::body(),
        PanelKind::Project => files::body(),
        PanelKind::Console => docks::console(),
        PanelKind::Custom(name) if name == HISTORY => docks::history(),
        PanelKind::Custom(_) => placeholder(),
    }
}

fn placeholder() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ThemeBackgroundColor(tokens::PANEL)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use eme::app::TaskPoolPlugin;
    use eme::asset::AssetPlugin;
    use eme::scene::ScenePlugin;

    use super::*;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin::default(),
            ScenePlugin,
        ))
        .init_asset::<Font>()
        .init_asset::<Image>();
        app
    }

    fn spawn_tree(app: &mut App, layout: Layout) {
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn_scene_list(tree(&layout));
        });
        app.update();
        app.update();
    }

    fn dock_ids(app: &mut App) -> HashSet<PanelId> {
        let world = app.world_mut();
        let mut query = world.query::<&Dock>();
        query.iter(world).map(|dock| dock.id).collect()
    }

    #[test]
    fn every_panel_gets_one_dock() {
        let layout = Layout::default_editor();
        let want = layout.panels().len();
        let mut app = test_app();
        spawn_tree(&mut app, layout);

        let ids = dock_ids(&mut app);
        assert_eq!(ids.len(), want, "rendered dock count disagrees with panels");
    }

    #[test]
    fn ratio_becomes_the_first_flex_basis_percentage() {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let mut app = test_app();
            spawn_tree(
                &mut app,
                Layout::Split {
                    axis,
                    ratio: 0.7,
                    first: Box::new(Layout::Panel {
                        id: PanelId(1),
                        kind: PanelKind::Scene,
                    }),
                    second: Box::new(Layout::Panel {
                        id: PanelId(2),
                        kind: PanelKind::Console,
                    }),
                },
            );

            let world = app.world_mut();
            let mut query = world.query::<&Node>();
            let basis: Vec<Val> = query.iter(world).map(|node| node.flex_basis).collect();
            assert!(
                basis.contains(&percent(70.0)),
                "{axis:?} split lost its ratio: {basis:?}"
            );
        }
    }

    #[test]
    fn titles_cover_every_panel_kind() {
        assert_eq!(title(&PanelKind::Hierarchy), "Scene");
        assert_eq!(title(&PanelKind::Scene), "Viewport");
        assert_eq!(title(&PanelKind::Inspector), "Inspector");
        assert_eq!(title(&PanelKind::Project), "FileSystem");
        assert_eq!(title(&PanelKind::Console), "Console");
        assert_eq!(title(&PanelKind::Custom("Outline".into())), "Outline");
        assert_eq!(title(&PanelKind::Custom("Stats".into())), "Stats");
    }

    #[derive(Resource, Default)]
    struct Tally(usize);

    fn tally_docks(docks: Query<(), Added<Dock>>, mut tally: ResMut<Tally>) {
        tally.0 += docks.iter().count();
    }

    #[test]
    fn a_layout_change_rebuilds_the_tree_once() {
        let mut app = test_app();
        app.insert_resource(LayoutRes(Layout::default_editor()))
            .init_resource::<Tally>()
            .add_systems(Update, (sync, tally_docks));
        app.world_mut().spawn(DockRoot);

        app.update();
        app.update();
        let first = app.world().resource::<Tally>().0;
        assert_eq!(first, Layout::default_editor().panels().len());

        app.update();
        assert_eq!(
            app.world().resource::<Tally>().0,
            first,
            "an untouched layout rebuilt the tree"
        );

        app.world_mut()
            .resource_mut::<LayoutRes>()
            .0
            .set_ratio(PanelId(3), 0.5);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<Tally>().0,
            first * 2,
            "one layout change must rebuild exactly once"
        );
    }
}
