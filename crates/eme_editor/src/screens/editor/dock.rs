//! Docking frame built from the editor [`Layout`].
//!
//! Every leaf owns one [`Dock`] with the dock id the layout assigned. Each tab
//! carries its own [`PanelId`] so a drag can move it between docks without
//! re-deriving identity from node position. [`DockRoot`] marks the node whose
//! children are replaced when the layout changes.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor};
use eme::math::Rect;
use eme::prelude::*;
use eme::scene::bsn;
use eme::window::{PrimaryWindow, Window};
use eme_editor_core::{Axis, Edge, Layout, PanelId, PanelKind, PanelTab};

use super::chrome::{self, splitter, tab, tab_strip, Part, STATUS_H};
use super::{docks, files, icons, inspector, viewport};
use crate::tokens;
use crate::LayoutRes;

/// Panel name understood as the fixed chrome strip rather than a resizable dock.
const STATUS: &str = "Status";

/// Panel name that renders the undo history content.
const HISTORY: &str = "History";

/// Pointer travel, in logical pixels, before a press on a tab becomes a drag.
const DRAG_SLOP: f32 = 4.0;

/// Fraction of a dock edge that accepts a split drop.
const EDGE_BAND: f32 = 0.25;

/// Host of the dock tree; its children are rebuilt from [`Layout`].
#[derive(Component, Clone, Copy, Default)]
pub struct DockRoot;

/// Identity of one leaf dock and the drop target for a tab.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dock {
    /// Dock this leaf renders.
    pub id: PanelId,
}

impl Default for Dock {
    fn default() -> Self {
        Self { id: PanelId(0) }
    }
}

/// One tab inside a dock.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockTab {
    /// Dock that owns the tab.
    pub dock: PanelId,
    /// Identifier of the tab.
    pub id: PanelId,
    /// Index in the dock's tab strip.
    pub index: usize,
}

impl Default for DockTab {
    fn default() -> Self {
        Self {
            dock: PanelId(0),
            id: PanelId(0),
            index: 0,
        }
    }
}

/// The translucent drop highlight.
#[derive(Component, Clone, Copy, Default)]
pub struct DropOverlay;

/// The tab strip of a dock, an explicit merge target.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockStrip {
    /// Dock whose tabs the strip shows.
    pub dock: PanelId,
}

impl Default for DockStrip {
    fn default() -> Self {
        Self { dock: PanelId(0) }
    }
}

/// Where a dragged tab came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabOrigin {
    dock: PanelId,
    tab: PanelId,
}

/// What a release over a dock does with the dragged tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropZone {
    /// Join the dock's tab stack.
    Merge,
    /// Become a new split on this side of the dock.
    Split(Edge),
}

/// A dock and the action a release would take there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DropTarget {
    dock: PanelId,
    zone: DropZone,
}

/// Live tab drag state.
#[derive(Resource, Default)]
struct TabDrag {
    source: Option<TabOrigin>,
    pointer_offset: Vec2,
    active: bool,
}

impl TabDrag {
    fn clear(&mut self) {
        self.source = None;
        self.pointer_offset = Vec2::ZERO;
        self.active = false;
    }
}

/// Installs the tab drag resource and its frame systems.
pub fn plugins(app: &mut App) {
    app.init_resource::<TabDrag>()
        .add_systems(Update, (sync_drop_feedback, cancel_on_escape));
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

/// The absolute drop highlight, a sibling of the dock host in the editor frame.
pub fn overlay() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(0),
            top: px(0),
            width: px(0),
            height: px(0),
            border: px(1),
            border_radius: px(2),
        }
        DropOverlay
        GlobalZIndex(150)
        Pickable::IGNORE
        ThemeBackgroundColor(tokens::DROP)
        ThemeBorderColor(tokens::ACCENT)
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
        Layout::Panel { id, tabs, active } => dock(*id, tabs, *active),
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
    let Layout::Panel { tabs, active, .. } = layout else {
        return None;
    };
    let tab = tabs.get(*active)?;
    matches!(&tab.kind, PanelKind::Custom(name) if name == STATUS).then_some(STATUS_H)
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

fn dock(id: PanelId, tabs: &[PanelTab], active: usize) -> Part {
    if tabs.is_empty() {
        return placeholder();
    }
    let active = active.min(tabs.len() - 1);
    let active_tab = &tabs[active];

    if matches!(&active_tab.kind, PanelKind::Custom(name) if name == STATUS) {
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

    let strip: Vec<Part> = tabs
        .iter()
        .enumerate()
        .map(|(index, entry)| tab_part(id, index, entry, index == active))
        .collect();
    let strip = strip_part(id, tab_strip(strip, tail(&active_tab.kind), tokens::WINDOW));

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
            {body(&active_tab.kind)}
        ]
    })
}

fn strip_part(dock: PanelId, strip: Part) -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        DockStrip { dock: {dock} }
        Children [ {strip} ]
    })
}

fn tab_part(dock: PanelId, index: usize, entry: &PanelTab, active: bool) -> Part {
    let label = title(&entry.kind);
    let inner = tab(&label, active);
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
        }
        DockTab { dock: {dock}, id: {entry.id}, index: {index} }
        Children [ {inner} ]
        on(select_tab)
        on(start_tab_drag)
        on(drag_tab)
        on(end_tab_drag)
        on(cancel_tab_drag)
    })
}

fn select_tab(
    mut click: On<PointerClick>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    mut layout: ResMut<LayoutRes>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Some((_, hit)) = find_tab(click.entity, &tabs, &parents) else {
        return;
    };
    click.propagate(false);
    layout.set_active(hit.dock, hit.index);
}

fn start_tab_drag(
    start: On<PointerDragStart>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut drag: ResMut<TabDrag>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let Some((entity, hit)) = find_tab(start.entity, &tabs, &parents) else {
        return;
    };
    let Ok((node, global)) = nodes.get(entity) else {
        return;
    };
    let inv = node.inverse_scale_factor();
    let rect = Rect::from_center_size(global.translation.trunc() * inv, node.size() * inv);

    drag.source = Some(TabOrigin {
        dock: hit.dock,
        tab: hit.id,
    });
    drag.pointer_offset = start.pointer.position - rect.min;
    drag.active = false;
}

fn drag_tab(drag: On<PointerDrag>, mut state: ResMut<TabDrag>) {
    if drag.button != PointerButton::Primary || state.source.is_none() {
        return;
    }
    if !state.active && drag.distance.length() >= DRAG_SLOP {
        state.active = true;
    }
}

fn end_tab_drag(
    end: On<PointerDragEnd>,
    docks: Query<(&Dock, &ComputedNode, &UiGlobalTransform)>,
    strips: Query<(&DockStrip, &ComputedNode, &UiGlobalTransform)>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut drag: ResMut<TabDrag>,
    mut layout: ResMut<LayoutRes>,
) {
    if end.button != PointerButton::Primary {
        return;
    }
    let source = drag.source.take();
    let active = drag.active;
    drag.active = false;

    if !active {
        return;
    }
    let Some(source) = source else {
        return;
    };
    let Some(pos) = window.cursor_position() else {
        return;
    };
    let at = pos * window.scale_factor();
    let rects = dock_rects(&docks);
    let strips = strip_rects(&strips);
    let Some((target, _)) = target_under(&layout, source, at, &rects, &strips) else {
        return;
    };
    let Some(tab) = layout.detach(source.tab) else {
        return;
    };

    let moved = match target.zone {
        DropZone::Merge => layout.drop_as_tab(target.dock, tab.clone()),
        DropZone::Split(edge) => layout.drop_as_split(target.dock, edge, tab.clone()),
    };
    if moved {
        persist(&layout);
    } else {
        layout.drop_as_tab(source.dock, tab);
    }
}

fn cancel_tab_drag(_cancel: On<PointerCancel>, mut drag: ResMut<TabDrag>) {
    drag.clear();
}

fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut drag: ResMut<TabDrag>,
) {
    if drag.source.is_some() && (keys.just_pressed(KeyCode::Escape) || !window.focused) {
        drag.clear();
    }
}

fn find_tab(
    mut entity: Entity,
    tabs: &Query<&DockTab>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, DockTab)> {
    loop {
        if let Ok(hit) = tabs.get(entity) {
            return Some((entity, *hit));
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

fn sync_drop_feedback(
    window: Single<&Window, With<PrimaryWindow>>,
    docks: Query<(&Dock, &ComputedNode, &UiGlobalTransform)>,
    strips: Query<(&DockStrip, &ComputedNode, &UiGlobalTransform)>,
    layout: Res<LayoutRes>,
    drag: Res<TabDrag>,
    mut overlays: Query<&mut Node, With<DropOverlay>>,
) {
    let Ok(mut node) = overlays.single_mut() else {
        return;
    };
    let Some(source) = drag.source else {
        hide_overlay(&mut node);
        return;
    };
    if !drag.active {
        hide_overlay(&mut node);
        return;
    }
    let Some(pos) = window.cursor_position() else {
        hide_overlay(&mut node);
        return;
    };

    let at = pos * window.scale_factor();
    let rects = dock_rects(&docks);
    let strips = strip_rects(&strips);
    let Some((target, rect)) = target_under(&layout, source, at, &rects, &strips) else {
        hide_overlay(&mut node);
        return;
    };

    let scale = window.scale_factor();
    let rect = Rect {
        min: rect.min / scale,
        max: rect.max / scale,
    };
    let size = rect.size();
    let (min, extent) = match target.zone {
        DropZone::Merge => (rect.min, size),
        DropZone::Split(Edge::Left) => (rect.min, Vec2::new(size.x / 2.0, size.y)),
        DropZone::Split(Edge::Right) => (
            Vec2::new(rect.min.x + size.x / 2.0, rect.min.y),
            Vec2::new(size.x / 2.0, size.y),
        ),
        DropZone::Split(Edge::Top) => (rect.min, Vec2::new(size.x, size.y / 2.0)),
        DropZone::Split(Edge::Bottom) => (
            Vec2::new(rect.min.x, rect.min.y + size.y / 2.0),
            Vec2::new(size.x, size.y / 2.0),
        ),
    };

    node.display = Display::Flex;
    node.left = px(min.x);
    node.top = px(min.y);
    node.width = px(extent.x);
    node.height = px(extent.y);
}

fn hide_overlay(node: &mut Node) {
    if node.display != Display::None {
        node.display = Display::None;
    }
}

fn dock_rects(docks: &Query<(&Dock, &ComputedNode, &UiGlobalTransform)>) -> Vec<(PanelId, Rect)> {
    docks
        .iter()
        .map(|(dock, node, global)| {
            (
                dock.id,
                Rect::from_center_size(global.translation.trunc(), node.size()),
            )
        })
        .collect()
}

fn strip_rects(
    strips: &Query<(&DockStrip, &ComputedNode, &UiGlobalTransform)>,
) -> Vec<(PanelId, Rect)> {
    strips
        .iter()
        .map(|(strip, node, global)| {
            (
                strip.dock,
                Rect::from_center_size(global.translation.trunc(), node.size()),
            )
        })
        .collect()
}

fn target_under(
    layout: &Layout,
    source: TabOrigin,
    at: Vec2,
    rects: &[(PanelId, Rect)],
    strips: &[(PanelId, Rect)],
) -> Option<(DropTarget, Rect)> {
    if let Some((dock, strip_rect)) = strips.iter().find(|(_, rect)| rect.contains(at)).copied()
        && can_drop(layout, source, dock, DropZone::Merge)
    {
        let rect = rects
            .iter()
            .find(|(id, _)| *id == dock)
            .map_or(strip_rect, |(_, rect)| *rect);
        return Some((
            DropTarget {
                dock,
                zone: DropZone::Merge,
            },
            rect,
        ));
    }

    let (dock, rect) = rects.iter().find(|(_, rect)| rect.contains(at)).copied()?;
    let zone = drop_zone(rect, at)?;
    can_drop(layout, source, dock, zone).then_some((DropTarget { dock, zone }, rect))
}

fn can_drop(layout: &Layout, source: TabOrigin, dock: PanelId, zone: DropZone) -> bool {
    if dock != source.dock {
        return layout.tabs(dock).is_some();
    }
    match zone {
        DropZone::Merge => false,
        DropZone::Split(_) => layout.tab_count(dock).is_some_and(|count| count > 1),
    }
}

fn drop_zone(rect: Rect, point: Vec2) -> Option<DropZone> {
    if !rect.contains(point) {
        return None;
    }
    let size = rect.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let local = (point - rect.min) / size;
    if (0.5 - EDGE_BAND..=0.5 + EDGE_BAND).contains(&local.x)
        && (0.5 - EDGE_BAND..=0.5 + EDGE_BAND).contains(&local.y)
    {
        return Some(DropZone::Merge);
    }

    let candidates = [
        (Edge::Left, local.x),
        (Edge::Right, 1.0 - local.x),
        (Edge::Top, local.y),
        (Edge::Bottom, 1.0 - local.y),
    ];
    let mut best = candidates[0];
    for candidate in &candidates[1..] {
        if candidate.1 < best.1 {
            best = *candidate;
        }
    }
    Some(DropZone::Split(best.0))
}

fn persist(layout: &Layout) {
    let path = crate::asset_path(crate::LAYOUT_PATH);
    let text = match ron::ser::to_string_pretty(
        layout,
        ron::ser::PrettyConfig::default().struct_names(true),
    ) {
        Ok(text) => text,
        Err(err) => {
            error!("could not serialize {}: {err}", crate::LAYOUT_PATH);
            return;
        }
    };
    if let Err(err) = std::fs::write(&path, text) {
        error!("could not write {}: {err}", path.display());
    }
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

        let tab = layout.detach(PanelId(14)).unwrap();
        layout.drop_as_tab(PanelId(3), tab);
        assert!(can_drop(
            &layout,
            origin,
            PanelId(3),
            DropZone::Split(Edge::Left)
        ));
    }
}
