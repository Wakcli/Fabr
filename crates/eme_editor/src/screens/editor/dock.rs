//! Docking frame built from the editor [`Layout`].
//!
//! Every leaf owns one [`Dock`] with the dock id the layout assigned. Each tab
//! carries its own [`PanelId`] so a drag can move it between docks without
//! re-deriving identity from node position. [`DockRoot`] marks the node whose
//! children are replaced when the layout changes.

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, UiTheme};
use eme::math::Rect;
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{FontSize, FontSourceTemplate, FontWeight, TextFont};
use eme::ui::widget::Text;
use eme::ui_widgets::Button;
use eme::window::{PrimaryWindow, Window};
use eme_editor_core::{Axis, Edge, Layout, PanelId, PanelKind, PanelTab, MAX_RATIO, MIN_RATIO};

use super::chrome::{self, tab, tab_strip, Part, FONT_SIZE, STATUS_H};
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

/// Horizontal offset from the pointer to the floating drag tab.
const GHOST_DX: f32 = 14.0;

/// Vertical offset from the pointer to the floating drag tab.
const GHOST_DY: f32 = 10.0;

/// Width of the insertion bar drawn between two tabs.
const CARET_W: f32 = 2.0;

/// Host of the dock tree; its children are rebuilt from [`Layout`].
#[derive(Component, Clone, Copy, Default)]
pub struct DockRoot;

/// Marks the first child of a split so a dragged divider can resize it live.
#[derive(Component, Clone, Copy, Default)]
pub struct SplitFirst(pub usize);

/// A draggable divider between the two children of the split at `index`.
#[derive(Component, Clone, Copy, Default)]
pub struct SplitHandle {
    /// Depth-first index of the owning split.
    pub index: usize,
    /// True when the split lays its children out side by side.
    pub horizontal: bool,
}

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

/// Vertical accent bar marking the slot a dragged tab would take.
#[derive(Component, Clone, Copy, Default)]
pub struct InsertCaret;

/// The floating tab that follows the pointer during a drag.
#[derive(Component, Clone, Copy, Default)]
pub struct DragGhost;

/// Title text of the floating drag tab.
#[derive(Component, Clone, Copy, Default)]
pub struct DragGhostLabel;

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
    /// Join the dock's tab stack at the end.
    Merge,
    /// Become a new split on this side of the dock.
    Split(Edge),
    /// Take the tab slot at `index` in `dock`.
    Insert { dock: PanelId, index: usize },
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
        .init_resource::<SplitDrag>().add_systems(
        Update,
        (
            sync_drop_feedback,
            sync_drag_ghost,
            sync_drag_source,
            cancel_on_escape,
        ),
    );
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
        Children [ {content(layout, &mut 0)} ]
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
            border: px(2),
            border_radius: px(4),
        }
        DropOverlay
        GlobalZIndex(150)
        Pickable::IGNORE
        ThemeBackgroundColor(tokens::DROP)
        ThemeBorderColor(tokens::ACCENT)
    })
}

/// The absolute bar showing the exact slot a dragged tab would take.
pub fn insert_caret() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(0),
            top: px(0),
            width: px(CARET_W),
            height: px(0),
        }
        InsertCaret
        GlobalZIndex(151)
        Pickable::IGNORE
        ThemeBackgroundColor(tokens::ACCENT)
    })
}

/// The floating tab that follows the pointer while a drag is live.
pub fn drag_ghost() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(0),
            top: px(0),
            height: px(chrome::DOCK_TAB_H),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { left: px(11), right: px(11), top: px(5), bottom: px(5) },
            border: UiRect { left: px(1), right: px(1), top: px(1), bottom: px(1) },
            border_radius: px(2),
        }
        DragGhost
        GlobalZIndex(160)
        Pickable::IGNORE
        ThemeBackgroundColor(tokens::GHOST)
        ThemeBorderColor(tokens::GHOST_BORDER)
        Children [
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
            }
            Children [
                Text("")
                TextFont {
                    font: FontSourceTemplate::Handle(fonts::REGULAR),
                    font_size: FontSize::Px(FONT_SIZE),
                    weight: FontWeight::NORMAL,
                }
                ThemeTextColor(tokens::TEXT)
                DragGhostLabel
            ]
            --
            Node { height: px(2) }
            ThemeBackgroundColor(tokens::ACCENT)
        ]
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
        Children [ {content(&layout, &mut 0)} ]
    });
}

fn content(layout: &Layout, hits: &mut usize) -> Part {
    match layout {
        Layout::Split {
            axis,
            ratio,
            first,
            second,
        } => split(*axis, *ratio, first, second, hits),
        Layout::Panel { id, tabs, active } => dock(*id, tabs, *active),
    }
}

fn handle(axis: Axis, index: usize) -> Part {
    let horizontal = axis == Axis::Horizontal;
    let (w, h) = if horizontal {
        (px(5), Val::Auto)
    } else {
        (Val::Auto, px(5))
    };
    let (line_w, line_h) = if horizontal {
        (px(1), percent(100))
    } else {
        (percent(100), px(1))
    };
    Box::new(bsn! {
        Node {
            width: {w},
            height: {h},
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        Button
        SplitHandle { index, horizontal }
        Children [
            Node { width: {line_w}, height: {line_h} }
            ThemeBackgroundColor(tokens::BORDER)
        ]
        on(start_split_drag)
        on(drag_split)
        on(end_split_drag)
    })
}

/// Divider drag in flight.
#[derive(Resource, Default)]
pub struct SplitDrag {
    index: usize,
    horizontal: bool,
    origin: Vec2,
    total: f32,
    start_px: f32,
    ratio: f32,
    active: bool,
}

fn start_split_drag(
    start: On<PointerDragStart>,
    handles: Query<(&SplitHandle, &ChildOf)>,
    containers: Query<&ComputedNode>,
    children: Query<(&SplitFirst, &ComputedNode)>,
    mut drag: ResMut<SplitDrag>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok((handle, child_of)) = handles.get(start.entity) else {
        return;
    };
    let Ok(container) = containers.get(child_of.parent()) else {
        return;
    };
    let size = container.size();
    let total = if handle.horizontal { size.x } else { size.y };
    if total <= 0.0 {
        return;
    }
    let start_px = children
        .iter()
        .find(|(first, _)| first.0 == handle.index)
        .map(|(_, node)| {
            let size = node.size();
            if handle.horizontal { size.x } else { size.y }
        })
        .unwrap_or(0.0);
    *drag = SplitDrag {
        index: handle.index,
        horizontal: handle.horizontal,
        origin: start.pointer.position,
        total,
        start_px,
        ratio: start_px / total,
        active: false,
    };
}

fn drag_split(
    drag_ev: On<PointerDrag>,
    mut drag: ResMut<SplitDrag>,
    mut children: Query<(&SplitFirst, &mut Node)>,
) {
    if drag_ev.button != PointerButton::Primary || drag.total <= 0.0 {
        return;
    }
    let pointer = drag_ev.pointer.position;
    if !drag.active {
        if (pointer - drag.origin).length() < DRAG_SLOP {
            return;
        }
        drag.active = true;
    }
    let delta = if drag.horizontal {
        pointer.x - drag.origin.x
    } else {
        pointer.y - drag.origin.y
    };
    let ratio = ((drag.start_px + delta) / drag.total).clamp(MIN_RATIO, MAX_RATIO);
    drag.ratio = ratio;
    for (first, mut node) in &mut children {
        if first.0 == drag.index {
            node.flex_grow = 0.0;
            node.flex_shrink = 0.0;
            node.flex_basis = percent(ratio * 100.0);
        }
    }
}

fn end_split_drag(
    _: On<PointerDragEnd>,
    mut drag: ResMut<SplitDrag>,
    mut layout: ResMut<LayoutRes>,
) {
    if !drag.active {
        return;
    }
    drag.active = false;
    if layout.set_ratio_at(drag.index, drag.ratio) {
        persist(&layout);
    }
}

fn split(axis: Axis, ratio: f32, first: &Layout, second: &Layout, hits: &mut usize) -> Part {
    let index = *hits;
    *hits += 1;
    let dir = match axis {
        Axis::Horizontal => FlexDirection::Row,
        Axis::Vertical => FlexDirection::Column,
    };

    let (a, b) = match (fixed_extent(first), fixed_extent(second)) {
        (Some(h), _) => (
            sized(content(first, hits), Sizing::Fixed(h), None),
            sized(content(second, hits), Sizing::Grow, None),
        ),
        (None, Some(h)) => (
            sized(content(first, hits), Sizing::Grow, None),
            sized(content(second, hits), Sizing::Fixed(h), None),
        ),
        _ => (
            sized(content(first, hits), Sizing::Ratio(ratio * 100.0), Some(index)),
            sized(content(second, hits), Sizing::Grow, None),
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
        Children [ {a} -- {handle(axis, index)} -- {b} ]
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

fn sized(child: Part, sizing: Sizing, first: Option<usize>) -> Part {
    let (grow, flex_basis, shrink): (f32, Val, f32) = match sizing {
        Sizing::Ratio(share) => (0.0, percent(share), 0.0),
        Sizing::Fixed(extent) => (0.0, px(extent), 0.0),
        Sizing::Grow => (1.0, px(0), 1.0),
    };
    if let Some(index) = first {
        return Box::new(bsn! {
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
            SplitFirst(index)
            Children [ {child} ]
        });
    }
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
    tabs: Query<(&DockTab, &ComputedNode, &UiGlobalTransform)>,
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
    let tab_rects = collect_tab_rects(&tabs);
    let Some((target, _)) = target_under(&layout, source, at, &rects, &strips, &tab_rects) else {
        return;
    };
    let src_index = tab_index(&layout, source).unwrap_or(0);
    if let DropZone::Insert { dock, index } = target.zone
        && dock == source.dock
        && (index == src_index || index == src_index + 1)
    {
        return;
    }
    let Some(tab) = layout.detach(source.tab) else {
        return;
    };

    let moved = match target.zone {
        DropZone::Merge => layout.drop_as_tab(target.dock, tab.clone()),
        DropZone::Split(edge) => layout.drop_as_split(target.dock, edge, tab.clone()),
        DropZone::Insert { dock, index } => {
            let index = if dock == source.dock && src_index < index {
                index - 1
            } else {
                index
            };
            layout.drop_as_tab_at(dock, tab.clone(), index)
        }
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
    tabs: Query<(&DockTab, &ComputedNode, &UiGlobalTransform)>,
    layout: Res<LayoutRes>,
    drag: Res<TabDrag>,
    mut overlays: Query<&mut Node, (With<DropOverlay>, Without<InsertCaret>)>,
    mut carets: Query<&mut Node, (With<InsertCaret>, Without<DropOverlay>)>,
) {
    let (Ok(mut node), Ok(mut caret)) = (overlays.single_mut(), carets.single_mut()) else {
        return;
    };
    let Some(source) = drag.source else {
        hide_overlay(&mut node);
        hide_overlay(&mut caret);
        return;
    };
    if !drag.active {
        hide_overlay(&mut node);
        hide_overlay(&mut caret);
        return;
    }
    let Some(pos) = window.cursor_position() else {
        hide_overlay(&mut node);
        hide_overlay(&mut caret);
        return;
    };

    let at = pos * window.scale_factor();
    let rects = dock_rects(&docks);
    let strips = strip_rects(&strips);
    let tab_rects = collect_tab_rects(&tabs);
    let Some((target, rect)) = target_under(&layout, source, at, &rects, &strips, &tab_rects)
    else {
        hide_overlay(&mut node);
        hide_overlay(&mut caret);
        return;
    };

    if let DropZone::Insert { dock, index } = target.zone {
        hide_overlay(&mut node);
        let scale = window.scale_factor();
        let slots: Vec<Rect> = tab_rects
            .iter()
            .filter(|(id, _, _)| *id == dock)
            .map(|(_, _, rect)| *rect)
            .collect();
        let x = insert_edge(index, &slots, rect);
        caret.display = Display::Flex;
        caret.left = px(x / scale - CARET_W / 2.0);
        caret.top = px(rect.min.y / scale);
        caret.width = px(CARET_W);
        caret.height = px(rect.size().y / scale);
        return;
    }

    hide_overlay(&mut caret);
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
        DropZone::Insert { .. } => unreachable!("insert returned before the rect preview"),
    };

    node.display = Display::Flex;
    node.left = px(min.x);
    node.top = px(min.y);
    node.width = px(extent.x);
    node.height = px(extent.y);
}

fn sync_drag_ghost(
    window: Single<&Window, With<PrimaryWindow>>,
    layout: Res<LayoutRes>,
    drag: Res<TabDrag>,
    mut ghosts: Query<&mut Node, With<DragGhost>>,
    mut labels: Query<&mut Text, With<DragGhostLabel>>,
) {
    let Ok(mut node) = ghosts.single_mut() else {
        return;
    };
    let Some(source) = drag.source.filter(|_| drag.active) else {
        hide_overlay(&mut node);
        return;
    };
    let Some(pos) = window.cursor_position() else {
        hide_overlay(&mut node);
        return;
    };

    node.display = Display::Flex;
    node.left = px(pos.x + GHOST_DX);
    node.top = px(pos.y + GHOST_DY);

    let Some(tab) = layout.find_tab(source.tab) else {
        return;
    };
    let label = title(&tab.kind);
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

fn sync_drag_source(
    theme: Res<UiTheme>,
    layout: Res<LayoutRes>,
    drag: Res<TabDrag>,
    mut tabs: Query<(Entity, &DockTab)>,
    children: Query<&Children>,
    mut fills: Query<&mut BackgroundColor>,
    mut texts: Query<&mut TextColor>,
) {
    if !drag.is_changed() {
        return;
    }
    let source = drag.source.filter(|_| drag.active);
    for (entity, tab) in &mut tabs {
        let dragged = source.is_some_and(|source| source.dock == tab.dock && source.tab == tab.id);
        let active = layout
            .tabs(tab.dock)
            .and_then(|tabs| tabs.get(tab.index))
            .is_some_and(|entry| entry.id == tab.id);

        let (fill, text) = if dragged {
            (tokens::CLEAR, tokens::TEXT_MEDIUM)
        } else if active {
            (tokens::PANEL, tokens::TEXT)
        } else {
            (tokens::WINDOW, tokens::TEXT_DIM)
        };

        let Ok(kids) = children.get(entity) else {
            continue;
        };
        let Some(&face) = kids.first() else {
            continue;
        };
        if let Ok(mut bg) = fills.get_mut(face) {
            bg.0 = theme.color(&fill);
        }
        let Ok(face_kids) = children.get(face) else {
            continue;
        };
        let Some(&label) = face_kids.first() else {
            continue;
        };
        if let Ok(mut color) = texts.get_mut(label) {
            color.0 = theme.color(&text);
        }
    }
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

fn collect_tab_rects(
    tabs: &Query<(&DockTab, &ComputedNode, &UiGlobalTransform)>,
) -> Vec<(PanelId, usize, Rect)> {
    let mut out: Vec<(PanelId, usize, Rect)> = tabs
        .iter()
        .map(|(tab, node, global)| {
            (
                tab.dock,
                tab.index,
                Rect::from_center_size(global.translation.trunc(), node.size()),
            )
        })
        .collect();
    out.sort_by_key(|(dock, index, _)| (dock.0, *index));
    out
}

fn insert_index(pointer_x: f32, tabs: &[Rect]) -> usize {
    tabs.iter()
        .position(|rect| pointer_x < rect.center().x)
        .unwrap_or(tabs.len())
}

fn insert_edge(index: usize, tabs: &[Rect], strip: Rect) -> f32 {
    match tabs.get(index) {
        Some(rect) => rect.min.x,
        None => tabs.last().map_or(strip.min.x, |rect| rect.max.x),
    }
}

fn tab_index(layout: &Layout, origin: TabOrigin) -> Option<usize> {
    layout
        .tabs(origin.dock)?
        .iter()
        .position(|tab| tab.id == origin.tab)
}

fn target_under(
    layout: &Layout,
    source: TabOrigin,
    at: Vec2,
    rects: &[(PanelId, Rect)],
    strips: &[(PanelId, Rect)],
    tabs: &[(PanelId, usize, Rect)],
) -> Option<(DropTarget, Rect)> {
    if let Some((dock, strip_rect)) = strips.iter().find(|(_, rect)| rect.contains(at)).copied() {
        let slots: Vec<Rect> = tabs
            .iter()
            .filter(|(id, _, _)| *id == dock)
            .map(|(_, _, rect)| *rect)
            .collect();
        let zone = DropZone::Insert {
            dock,
            index: insert_index(at.x, &slots),
        };
        if can_drop(layout, source, dock, zone) {
            return Some((DropTarget { dock, zone }, strip_rect));
        }
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
        DropZone::Insert { .. } => layout.tab_count(dock).is_some(),
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
        PanelKind::Custom(name) if name == "Stats" => docks::stats(),
        PanelKind::Custom(name) if name == "Outline" => docks::outline(),
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
mod tests;
