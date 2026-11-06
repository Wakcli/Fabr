use std::collections::{HashMap, HashSet};

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

fn tab_counts(app: &mut App) -> HashMap<u32, usize> {
    let world = app.world_mut();
    let mut query = world.query::<&DockTab>();
    let mut out = HashMap::new();
    for tab in query.iter(world) {
        *out.entry(tab.dock.0).or_default() += 1;
    }
    out
}

fn leaf(dock: u32, tab: u32, kind: PanelKind) -> Layout {
    Layout::Panel {
        id: PanelId(dock),
        tabs: vec![PanelTab {
            id: PanelId(tab),
            kind,
        }],
        active: 0,
    }
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
                first: Box::new(leaf(1, 9, PanelKind::Scene)),
                second: Box::new(leaf(2, 10, PanelKind::Console)),
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
        .set_ratio(PanelId(11), 0.5);
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<Tally>().0,
        first * 2,
        "one layout change must rebuild exactly once"
    );
}

#[test]
fn moving_a_tab_rebuilds_it_into_the_target_dock() {
    let mut app = test_app();
    app.insert_resource(LayoutRes(Layout::default_editor()))
        .add_systems(Update, sync);
    app.world_mut().spawn(DockRoot);
    app.update();
    app.update();

    let layout = &mut app.world_mut().resource_mut::<LayoutRes>().0;
    let tab = layout.detach(PanelId(14)).expect("inspector tab detaches");
    assert!(layout.drop_as_tab(PanelId(1), tab));

    app.update();
    app.update();

    let counts = tab_counts(&mut app);
    assert_eq!(counts.get(&1), Some(&2), "target dock keeps both tabs");
    assert_eq!(counts.get(&6), None, "emptied dock is gone");
    assert_eq!(counts.get(&3), Some(&1));
}

#[test]
fn drop_zone_centres_merge_and_bands_split() {
    let rect = Rect::from_center_size(Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));

    assert_eq!(
        drop_zone(rect, Vec2::new(100.0, 100.0)),
        Some(DropZone::Merge)
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(10.0, 100.0)),
        Some(DropZone::Split(Edge::Left))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(190.0, 100.0)),
        Some(DropZone::Split(Edge::Right))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(100.0, 60.0)),
        Some(DropZone::Split(Edge::Top))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(100.0, 140.0)),
        Some(DropZone::Split(Edge::Bottom))
    );
    assert_eq!(drop_zone(rect, Vec2::new(300.0, 100.0)), None);
}

#[test]
fn drop_zone_corners_resolve_consistently() {
    let rect = Rect::from_center_size(Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));

    assert_eq!(
        drop_zone(rect, Vec2::new(0.0, 50.0)),
        Some(DropZone::Split(Edge::Left))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(200.0, 150.0)),
        Some(DropZone::Split(Edge::Right))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(200.0, 50.0)),
        Some(DropZone::Split(Edge::Right))
    );
    assert_eq!(
        drop_zone(rect, Vec2::new(0.0, 150.0)),
        Some(DropZone::Split(Edge::Left))
    );
}

#[test]
fn insert_index_follows_the_pointer_across_the_strip() {
    let tabs = [
        Rect::from_corners(Vec2::new(0.0, 0.0), Vec2::new(100.0, 29.0)),
        Rect::from_corners(Vec2::new(100.0, 0.0), Vec2::new(200.0, 29.0)),
    ];

    assert_eq!(insert_index(-20.0, &tabs), 0);
    assert_eq!(insert_index(100.0, &tabs), 1);
    assert_eq!(insert_index(240.0, &tabs), 2);
    assert_eq!(insert_index(80.0, &[]), 0);
}

#[test]
fn strip_insert_wins_over_the_centre_and_edges() {
    let layout = Layout::default_editor();
    let source = TabOrigin {
        dock: PanelId(3),
        tab: PanelId(11),
    };
    let dock = Rect::from_corners(Vec2::ZERO, Vec2::new(200.0, 100.0));
    let tabs = [
        (
            PanelId(1),
            0,
            Rect::from_corners(Vec2::ZERO, Vec2::new(100.0, 100.0)),
        ),
        (
            PanelId(1),
            1,
            Rect::from_corners(Vec2::new(100.0, 0.0), Vec2::new(200.0, 100.0)),
        ),
    ];

    // the strip covers the dock centre, which would otherwise merge
    let (target, rect) = target_under(
        &layout,
        source,
        Vec2::new(100.0, 50.0),
        &[(PanelId(1), dock)],
        &[(PanelId(1), dock)],
        &tabs,
    )
    .expect("strip target");
    assert_eq!(
        target.zone,
        DropZone::Insert {
            dock: PanelId(1),
            index: 1,
        }
    );
    assert_eq!(rect, dock);
}

#[test]
fn an_empty_strip_inserts_at_the_front() {
    let layout = Layout::default_editor();
    let source = TabOrigin {
        dock: PanelId(3),
        tab: PanelId(11),
    };
    let dock = Rect::from_corners(Vec2::ZERO, Vec2::new(200.0, 100.0));

    let (target, _) = target_under(
        &layout,
        source,
        Vec2::new(90.0, 20.0),
        &[(PanelId(1), dock)],
        &[(PanelId(1), dock)],
        &[],
    )
    .expect("strip target");
    assert_eq!(
        target.zone,
        DropZone::Insert {
            dock: PanelId(1),
            index: 0,
        }
    );
}

#[test]
fn self_merges_are_rejected_and_self_splits_need_a_second_tab() {
    let mut layout = Layout::default_editor();
    let origin = TabOrigin {
        dock: PanelId(3),
        tab: PanelId(11),
    };

    assert!(!can_drop(&layout, origin, PanelId(3), DropZone::Merge));
    assert!(!can_drop(
        &layout,
        origin,
        PanelId(3),
        DropZone::Split(Edge::Left)
    ));
    assert!(can_drop(&layout, origin, PanelId(1), DropZone::Merge));
    assert!(can_drop(
        &layout,
        origin,
        PanelId(3),
        DropZone::Insert {
            dock: PanelId(3),
            index: 0,
        }
    ));

    let tab = layout.detach(PanelId(14)).unwrap();
    layout.drop_as_tab(PanelId(3), tab);
    assert!(can_drop(
        &layout,
        origin,
        PanelId(3),
        DropZone::Split(Edge::Left)
    ));
}
