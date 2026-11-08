//! The Scene, `FileSystem`, Output, and History docks.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken};
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, InputFocus};
use eme::math::Rect;
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, ScrollArea, TextInput};
use eme::window::{PrimaryWindow, Window};
use eme_game_overrides::{ModelNodeCatalog, ModelNodeInfo};
use eme_game_scene::{NodeKey, ObjectKind, OverrideOp, SceneDoc};

use super::chrome::*;
use super::icons;
use super::menu::{self, MenuState};
use crate::scene::{ModelOwner, NodeSelection, ReloadModel, SceneState};
use eme::ecs::entity::Entities;
use eme::time::Time;
use crate::tokens;
use crate::undo::Undo;

/// Marks the scene-tree body so a model drop can find it.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneTreePanel;

/// Hierarchy panel body: the scene tree toolbar, filter, and rows.
pub fn hierarchy() -> Part {
    let toolbar = Box::new(bsn! {
        Node {
            height: px(TOOL_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(4)),
            column_gap: px(1),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {menu::add_button()}
            --
            {icon_button(icons::instance(), 24.0)}
            --
            {icon_button(icons::attach_script(), 24.0)}
            --
            {spacer()}
            --
            {icon_button(icons::search(), 24.0)}
        ]
    });

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
        SceneFilterInput
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        on(focus_filter)
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
        SceneFilterHint
        Pickable::IGNORE
        Children [ {text("Filter nodes", FONT_SIZE, tokens::TEXT_DIM)} ]
    });

    let filter = Box::new(bsn! {
        Node {
            padding: UiRect {
                left: px(6),
                right: px(6),
                top: px(4),
                bottom: px(5),
            },
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            {field(vec![icons::search_dim(), input, hint], FIELD_H)}
        ]
    });

    let tree = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { top: px(3) },
            overflow: Overflow::clip(),
        }
        SceneTreePanel
        ThemeBackgroundColor(tokens::INSET)
        on(open_tree_menu)
        Children [ {tree_rows()} ]
    });

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            {toolbar}
            --
            {filter}
            --
            {tree}
        ]
    })
}

fn tree_rows() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        SceneRows
    })
}

/// Host for the rows that mirror the viewport scene; its bounds are the tree's
/// drop-to-root area.
#[derive(Component, Clone, Default)]
pub struct SceneRows;

/// Text typed into the hierarchy filter.
#[derive(Resource, Clone, Default)]
pub struct SceneFilter {
    /// Case-insensitive substring matched against object names.
    pub query: String,
    /// Bumped whenever [`query`](Self::query) changes.
    pub rev: u64,
}

impl SceneFilter {
    /// Objects to show, or `None` when the filter is empty.
    ///
    /// A match keeps its whole ancestor chain so the tree stays navigable.
    pub fn visible(&self, doc: &SceneDoc) -> Option<HashSet<u64>> {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return None;
        }
        let mut ids = HashSet::new();
        for obj in &doc.objects {
            if !obj.name.to_lowercase().contains(&query) {
                continue;
            }
            ids.insert(obj.id);
            let mut parent = obj.parent;
            while let Some(id) = parent {
                if !ids.insert(id) {
                    break;
                }
                parent = doc.get(id).and_then(|obj| obj.parent);
            }
        }
        Some(ids)
    }
}

/// The hierarchy filter's text input.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneFilterInput;

/// Placeholder shown while the filter is empty.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneFilterHint;

/// Copies the filter field's text into [`SceneFilter`].
pub fn sync_scene_filter(
    mut filter: ResMut<SceneFilter>,
    inputs: Query<&EditableText, With<SceneFilterInput>>,
) {
    let Ok(input) = inputs.single() else {
        return;
    };
    let value = input.value().to_string();
    if value == filter.query {
        return;
    }
    filter.query = value;
    filter.rev = filter.rev.wrapping_add(1);
}

/// Hides the placeholder once the filter has text.
pub fn sync_filter_hint(
    filter: Res<SceneFilter>,
    mut hints: Query<&mut Node, With<SceneFilterHint>>,
) {
    let want = if filter.query.is_empty() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut hints {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Gives the filter field input focus when it is pressed.
fn focus_filter(
    press: On<PointerPress>,
    inputs: Query<(), With<SceneFilterInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !inputs.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

/// Scene-tree row pointing at one spawned object.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneRow(pub u64);

/// Scene-tree row for one node inside an instanced model.
#[derive(Component, Clone, PartialEq, Default)]
pub struct SceneNodeRow(pub u64, pub NodeKey);

/// Include/exclude checkbox on a model node row.
#[derive(Component, Clone, PartialEq, Default)]
pub struct NodeExclude(pub u64, pub NodeKey);

/// Disclosure triangle on a row that has children.
#[derive(Component, Clone, Copy, Default)]
pub struct SceneCaret(pub u64);

/// Left padding added for each level of nesting.
const INDENT: f32 = 14.0;
/// Square the disclosure triangle occupies, matching [`icons::small`].
const CARET: f32 = 12.0;
/// Pointer travel that turns a press on a row body into a hierarchy drag.
const DRAG_SLOP: f32 = 4.0;
/// Frames the click released at the end of a drag must not select for.
const DRAG_SETTLE: u8 = 2;

/// Scene-tree view state: collapsed subtrees and the drag in flight.
#[derive(Resource, Default)]
pub struct SceneTree {
    collapsed: HashSet<u64>,
    drag: Drag,
    signature: Option<u64>,
    host: Option<Entity>,
}

/// A drag that started on a row body and has not been released yet.
#[derive(Default)]
struct Drag {
    /// Objects the drop carries; empty when no drag is pending.
    ids: Vec<u64>,
    /// Set once the pointer travelled past [`DRAG_SLOP`].
    active: bool,
    /// Row the pointer is over and would receive the drop.
    target: Option<u64>,
    /// Countdown that swallows the click released by a drag.
    settle: u8,
}

impl SceneTree {
    fn is_collapsed(&self, id: u64) -> bool {
        self.collapsed.contains(&id)
    }

    fn toggle(&mut self, id: u64) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }

    fn expand(&mut self, id: u64) {
        self.collapsed.remove(&id);
    }

    /// Drops collapse entries for objects that are no longer in the document.
    fn prune(&mut self, doc: &SceneDoc) {
        self.collapsed.retain(|id| doc.get(*id).is_some());
    }
}

/// Visible rows in tree order: a node before its children, siblings in document
/// order, subtrees of collapsed nodes skipped.
fn tree_order(doc: &SceneDoc, collapsed: &HashSet<u64>) -> Vec<(u64, usize)> {
    let mut rows = Vec::new();
    for root in doc.roots() {
        push_subtree(doc, collapsed, root.id, 0, &mut rows);
    }
    rows
}

fn push_subtree(
    doc: &SceneDoc,
    collapsed: &HashSet<u64>,
    id: u64,
    depth: usize,
    rows: &mut Vec<(u64, usize)>,
) {
    rows.push((id, depth));
    if collapsed.contains(&id) {
        return;
    }
    for child in doc.children(id) {
        push_subtree(doc, collapsed, child.id, depth + 1, rows);
    }
}

/// Objects a drag from `id` carries: the whole selection when it holds `id`.
fn drag_set(state: &SceneState, id: u64) -> Vec<u64> {
    if state.is_selected(id) {
        state.selected_ids().collect()
    } else {
        vec![id]
    }
}

/// Whether `dragged` may be reparented onto `target` without making a cycle.
fn can_drop(doc: &SceneDoc, dragged: &[u64], target: u64) -> bool {
    doc.get(target).is_some()
        && !dragged.contains(&target)
        && !dragged.iter().any(|id| doc.is_ancestor(*id, target))
}

/// What releasing a drag at a point does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DropSpot {
    /// Onto a row that can take it.
    Row(u64),
    /// Onto empty tree space, which detaches the dragged objects.
    Root,
    /// Onto a row that would make a cycle or sits inside the dragged set.
    Invalid,
    /// Outside the tree.
    Outside,
}

fn drop_spot(
    doc: &SceneDoc,
    dragged: &[u64],
    rows: &[(u64, Rect)],
    body: Option<Rect>,
    at: Vec2,
) -> DropSpot {
    if let Some((id, _)) = rows.iter().find(|(_, rect)| rect.contains(at))
        && body.is_none_or(|rect| rect.contains(at))
    {
        return if can_drop(doc, dragged, *id) {
            DropSpot::Row(*id)
        } else {
            DropSpot::Invalid
        };
    }
    if body.is_some_and(|rect| rect.contains(at)) {
        DropSpot::Root
    } else {
        DropSpot::Outside
    }
}

/// Physical-pixel rects of the rows on screen.
fn row_rects(rows: &Query<(&SceneRow, &ComputedNode, &UiGlobalTransform)>) -> Vec<(u64, Rect)> {
    rows.iter()
        .map(|(row, node, global)| {
            (
                row.0,
                Rect::from_center_size(global.translation.trunc(), node.size()),
            )
        })
        .collect()
}

/// Physical-pixel bounds of the tree body.
fn host_rect(
    hosts: &Query<(Entity, &ComputedNode, &UiGlobalTransform), With<SceneRows>>,
) -> Option<Rect> {
    let (_, node, global) = hosts.single().ok()?;
    Some(Rect::from_center_size(
        global.translation.trunc(),
        node.size(),
    ))
}

/// The icon a scene kind shows in the tree.
fn kind_badge(kind: ObjectKind) -> Part {
    match kind {
        ObjectKind::Empty => icons::mono(icons::EMPTY, tokens::CLASS_NODE),
        ObjectKind::Cube | ObjectKind::Model => icons::badge(icons::MESH),
        ObjectKind::Sphere => icons::mono(icons::SPHERE, tokens::CLASS_MESH),
        ObjectKind::Cylinder => icons::mono(icons::CYLINDER, tokens::CLASS_MESH),
        ObjectKind::Plane => icons::mono(icons::PLANE, tokens::CLASS_MESH),
        ObjectKind::PointLight | ObjectKind::Sun => icons::badge(icons::LIGHT),
        ObjectKind::Camera => icons::badge(icons::CAMERA),
    }
}

/// Identity of the tree's contents: what the rows are built from, ignoring selection.
fn tree_signature(
    state: &SceneState,
    tree: &SceneTree,
    catalogues: &HashMap<u64, Vec<ModelNodeInfo>>,
    node_selection: &NodeSelection,
    filter: &SceneFilter,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    filter.query.hash(&mut hasher);
    filter.rev.hash(&mut hasher);
    state.doc.len().hash(&mut hasher);
    for obj in &state.doc.objects {
        obj.id.hash(&mut hasher);
        obj.name.hash(&mut hasher);
        obj.kind.index().hash(&mut hasher);
        obj.parent.hash(&mut hasher);
        format!("{:?}", obj.overrides).hash(&mut hasher);
    }
    let mut ids: Vec<u64> = catalogues.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        id.hash(&mut hasher);
        if let Some(nodes) = catalogues.get(&id) {
            for node in nodes {
                node.key.hash(&mut hasher);
                node.depth.hash(&mut hasher);
                node.present.hash(&mut hasher);
            }
        }
    }
    node_selection.object.hash(&mut hasher);
    node_selection.key.hash(&mut hasher);
    let mut collapsed: Vec<u64> = tree.collapsed.iter().copied().collect();
    collapsed.sort_unstable();
    collapsed.hash(&mut hasher);
    hasher.finish()
}

/// Background and text tokens for a row in the given selection state.
fn row_colors(selected: bool) -> (ThemeToken, ThemeToken) {
    if selected {
        (tokens::SELECT, tokens::SELECT_TEXT)
    } else {
        (tokens::CLEAR, tokens::TEXT)
    }
}

/// Points an in-flight drag at the row under the pointer, and cancels it when
/// the pointer leaves the window.
fn track_drag_target(
    tree: &mut SceneTree,
    state: &SceneState,
    hosts: &Query<(Entity, &ComputedNode, &UiGlobalTransform), With<SceneRows>>,
    layout: &Query<(&SceneRow, &ComputedNode, &UiGlobalTransform)>,
    at: Option<Vec2>,
) {
    if !tree.drag.active {
        tree.drag.target = None;
        return;
    }
    let Some(at) = at else {
        tree.drag = Drag::default();
        return;
    };
    let rects = row_rects(layout);
    let target = match drop_spot(&state.doc, &tree.drag.ids, &rects, host_rect(hosts), at) {
        DropSpot::Row(id) => Some(id),
        _ => None,
    };
    tree.drag.target = target;
}

/// Rebuilds the scene rows when the tree changes and repaints them when the
/// selection or a drag does, so a Ctrl+click never respawns the tree.
pub fn rebuild_scene_rows(
    mut commands: Commands,
    hosts: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<SceneRows>>,
    state: Res<SceneState>,
    filter: Res<SceneFilter>,
    tree: Option<ResMut<SceneTree>>,
    rows: Query<(Entity, &SceneRow, &ThemeBackgroundColor, &Children)>,
    node_rows: Query<(Entity, &SceneNodeRow, &ThemeBackgroundColor, &Children)>,
    texts: Query<&ThemeTextColor>,
    layout: Query<(&SceneRow, &ComputedNode, &UiGlobalTransform)>,
    catalogs: Query<(&ModelOwner, &ModelNodeCatalog)>,
    node_selection: Res<NodeSelection>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let Some(mut tree) = tree else {
        commands.init_resource::<SceneTree>();
        return;
    };
    let Ok((host, _, _)) = hosts.single() else {
        return;
    };
    tree.prune(&state.doc);
    tree.drag.settle = tree.drag.settle.saturating_sub(1);
    track_drag_target(
        &mut tree,
        &state,
        &hosts,
        &layout,
        window
            .cursor_position()
            .map(|pos| pos * window.scale_factor()),
    );
    paint_scene_rows(&state, &tree, &rows, &texts, &mut commands);
    paint_node_rows(&state, &node_selection, &node_rows, &texts, &mut commands);

    let mut catalogues: HashMap<u64, Vec<ModelNodeInfo>> = HashMap::new();
    for (owner, catalog) in &catalogs {
        catalogues.insert(owner.0, catalog.nodes.clone());
    }
    let sig = tree_signature(&state, &tree, &catalogues, &node_selection, &filter);
    if tree.host == Some(host) && tree.signature == Some(sig) {
        return;
    }
    tree.host = Some(host);
    tree.signature = Some(sig);
    commands.entity(host).despawn_children();

    let visible = filter.visible(&state.doc);
    let no_collapse = HashSet::new();
    let collapsed = if visible.is_some() {
        &no_collapse
    } else {
        &tree.collapsed
    };
    let order = tree_order(&state.doc, collapsed);

    if order.is_empty() {
        let message = if filter.query.trim().is_empty() {
            "No objects in scene"
        } else {
            "No matching objects"
        };
        let empty = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(host)
            Children [ {text(message, FONT_SIZE, tokens::TEXT_DIM)} ]
        };
        commands.spawn_scene(empty);
        return;
    }

    for (id, depth) in order {
        if visible
            .as_ref()
            .is_some_and(|ids| !ids.contains(&id))
        {
            continue;
        }
        let Some(obj) = state.doc.get(id) else {
            continue;
        };
        let (bg, fg) = row_colors(state.is_selected(id));
        let open = !tree.is_collapsed(id);
        let mut kids: Vec<Part> = Vec::new();
        if state.doc.children(id).next().is_some() {
            let caret = icons::caret_tinted(open, fg.clone());
            kids.push(Box::new(bsn! {
                Node {
                    width: px(CARET),
                    height: px(CARET),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                }
                SceneCaret(id)
                Children [ {caret} ]
                on(toggle_row)
            }));
        }
        kids.push(kind_badge(obj.kind));
        kids.push(text(obj.name.clone(), FONT_SIZE, fg));
        let row = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect {
                    left: px(8.0 + depth as f32 * INDENT),
                    right: px(6),
                },
                column_gap: px(6),
            }
            ThemeBackgroundColor(bg)
            Button
            SceneRow(id)
            ChildOf(host)
            Children [ {kids} ]
            on(select_row)
            on(open_row_menu)
            on(start_row_drag)
            on(drag_row)
            on(end_row_drag)
            on(cancel_row_drag)
        };
        commands.spawn_scene(row);

        if !tree.is_collapsed(id)
            && let Some(nodes) = catalogues.get(&id)
        {
            for info in nodes {
                spawn_node_row(
                    &mut commands,
                    host,
                    id,
                    depth + 1 + info.depth,
                    info,
                    &obj.overrides,
                    &node_selection,
                );
            }
        }
    }
}

/// True when the node or one of its ancestors is excluded.
fn node_excluded(overrides: &[eme_game_scene::NodeOverride], key: &NodeKey) -> bool {
    overrides.iter().any(|entry| {
        key.is_within(&entry.key)
            && matches!(entry.op, OverrideOp::SetExcluded { excluded: true })
    })
}

/// One row for a glTF node under its model object.
fn spawn_node_row(
    commands: &mut Commands,
    host: Entity,
    object: u64,
    depth: usize,
    info: &ModelNodeInfo,
    overrides: &[eme_game_scene::NodeOverride],
    selection: &NodeSelection,
) {
    let excluded = node_excluded(overrides, &info.key);
    let selected = selection.object == Some(object) && selection.key.as_ref() == Some(&info.key);
    let (bg, fg) = if selected {
        (tokens::SELECT, tokens::SELECT_TEXT)
    } else if excluded || !info.present {
        (tokens::CLEAR, tokens::TEXT_DIM)
    } else {
        (tokens::CLEAR, tokens::TEXT)
    };
    let exclude_key = info.key.clone();
    let row_key = info.key.clone();
    let fill = if excluded { tokens::CLEAR } else { tokens::ACCENT };
    let mark = Box::new(bsn! {
        Node {
            width: px(10),
            height: px(10),
            border: UiRect::all(px(1)),
            border_radius: px(2),
        }
        ThemeBackgroundColor(fill)
        ThemeBorderColor(tokens::BORDER)
    }) as Part;
    let box_part = Box::new(bsn! {
        Node {
            width: px(16),
            height: px(16),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        NodeExclude(object, exclude_key)
        Children [ {mark} ]
        on(toggle_node_excluded)
    }) as Part;
    let row = bsn! {
        Node {
            height: px(TREE_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(8.0 + depth as f32 * INDENT),
                right: px(6),
            },
            column_gap: px(6),
        }
        ThemeBackgroundColor(bg)
        SceneNodeRow(object, row_key)
        ChildOf(host)
        Children [
            {box_part}
            --
            {icons::badge(icons::NODE)}
            --
            {text(info.key.name.clone(), FONT_SIZE, fg)}
        ]
        on(select_node_row)
    };
    commands.spawn_scene(row);
}

/// Clicking a node row selects the node and its owning object.
fn select_node_row(
    click: On<PointerClick>,
    rows: Query<&SceneNodeRow>,
    mut state: ResMut<SceneState>,
    mut node: ResMut<NodeSelection>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    state.select_only(row.0);
    node.object = Some(row.0);
    node.key = Some(row.1.clone());
}

/// Flips a node's include checkbox and stores it as an exclusion override.
fn toggle_node_excluded(
    mut click: On<PointerClick>,
    boxes: Query<&NodeExclude>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut commands: Commands,
    owners: Query<(&ModelOwner, Entity)>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(toggle) = boxes.get(click.entity) else {
        return;
    };
    click.propagate(false);
    let excluded = state.doc.overrides(toggle.0).iter().any(|entry| {
        entry.key == toggle.1 && matches!(entry.op, OverrideOp::SetExcluded { excluded: true })
    });
    undo.checkpoint(&state, "Exclude Node");
    state.doc.set_override(
        toggle.0,
        toggle.1.clone(),
        OverrideOp::SetExcluded {
            excluded: !excluded,
        },
    );
    state.touch();
    if excluded
        && let Some((_, entity)) = owners.iter().find(|(owner, _)| owner.0 == toggle.0)
    {
        commands.entity(entity).insert(ReloadModel);
    }
}

/// Repaints node rows from the node selection.
fn paint_node_rows(
    state: &SceneState,
    selection: &NodeSelection,
    rows: &Query<(Entity, &SceneNodeRow, &ThemeBackgroundColor, &Children)>,
    texts: &Query<&ThemeTextColor>,
    commands: &mut Commands,
) {
    for (entity, row, bg, kids) in rows {
        let selected = selection.object == Some(row.0)
            && selection.key.as_ref() == Some(&row.1);
        let excluded = node_excluded(state.doc.overrides(row.0), &row.1);
        let (want_bg, want_fg) = if selected {
            (tokens::SELECT, tokens::SELECT_TEXT)
        } else if excluded {
            (tokens::CLEAR, tokens::TEXT_DIM)
        } else {
            (tokens::CLEAR, tokens::TEXT)
        };
        if bg.0 != want_bg {
            commands.entity(entity).insert(ThemeBackgroundColor(want_bg));
        }
        for child in kids.iter() {
            if let Ok(current) = texts.get(child)
                && current.0 != want_fg
            {
                commands.entity(child).insert(ThemeTextColor(want_fg.clone()));
            }
        }
    }
}

/// Repaints rows whose selection or drop-target state no longer matches the screen.
fn paint_scene_rows(
    state: &SceneState,
    tree: &SceneTree,
    rows: &Query<(Entity, &SceneRow, &ThemeBackgroundColor, &Children)>,
    texts: &Query<&ThemeTextColor>,
    commands: &mut Commands,
) {
    let target = if tree.drag.active {
        tree.drag.target
    } else {
        None
    };
    for (entity, row, bg, kids) in rows {
        let (want_bg, want_fg) = if target == Some(row.0) {
            (tokens::ACCENT, tokens::SELECT_TEXT)
        } else {
            row_colors(state.is_selected(row.0))
        };
        if bg.0 != want_bg {
            commands
                .entity(entity)
                .insert(ThemeBackgroundColor(want_bg));
        }
        for child in kids.iter() {
            let Ok(current) = texts.get(child) else {
                continue;
            };
            if current.0 != want_fg {
                commands
                    .entity(child)
                    .insert(ThemeTextColor(want_fg.clone()));
            }
        }
    }
}

/// What a click on a row does to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowClick {
    Replace,
    Extend,
    Toggle,
}

/// Plain click replaces, Shift extends from the anchor, Ctrl toggles one row.
fn row_click(shift: bool, ctrl: bool) -> RowClick {
    if shift {
        RowClick::Extend
    } else if ctrl {
        RowClick::Toggle
    } else {
        RowClick::Replace
    }
}

fn select_row(
    activate: On<Activate>,
    keys: Res<ButtonInput<KeyCode>>,
    rows: Query<&SceneRow>,
    mut state: ResMut<SceneState>,
    mut tree: ResMut<SceneTree>,
) {
    if tree.drag.active || tree.drag.settle > 0 {
        tree.drag.settle = 0;
        return;
    }
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    match row_click(shift, ctrl) {
        RowClick::Replace => state.select_only(row.0),
        RowClick::Extend => state.extend_to(row.0),
        RowClick::Toggle => state.toggle(row.0),
    }
}

/// Clicking the disclosure triangle hides or shows one node's whole subtree.
fn toggle_row(
    mut click: On<PointerClick>,
    carets: Query<&SceneCaret>,
    mut tree: ResMut<SceneTree>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(caret) = carets.get(click.entity) else {
        return;
    };
    click.propagate(false);
    tree.toggle(caret.0);
}

/// A press on a row body arms a hierarchy drag; movement past [`DRAG_SLOP`] starts it.
fn start_row_drag(
    start: On<PointerDragStart>,
    rows: Query<&SceneRow>,
    state: Res<SceneState>,
    mut tree: ResMut<SceneTree>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = rows.get(start.entity) else {
        return;
    };
    tree.drag = Drag {
        ids: drag_set(&state, row.0),
        ..Drag::default()
    };
}

fn drag_row(drag: On<PointerDrag>, mut tree: ResMut<SceneTree>) {
    if drag.button != PointerButton::Primary || tree.drag.ids.is_empty() {
        return;
    }
    if !tree.drag.active && drag.distance.length() >= DRAG_SLOP {
        tree.drag.active = true;
    }
}

/// Window focus loss or a cancelled pointer abandons the drag without a drop.
fn cancel_row_drag(_cancel: On<PointerCancel>, mut tree: ResMut<SceneTree>) {
    tree.drag = Drag::default();
}

/// Drops a live drag on the row under the pointer, or detaches it on empty tree space.
fn end_row_drag(
    end: On<PointerDragEnd>,
    hosts: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<SceneRows>>,
    layout: Query<(&SceneRow, &ComputedNode, &UiGlobalTransform)>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut tree: ResMut<SceneTree>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
) {
    if !tree.drag.active {
        tree.drag.ids.clear();
        tree.drag.target = None;
        return;
    }
    let dragged = std::mem::take(&mut tree.drag.ids);
    tree.drag.active = false;
    tree.drag.target = None;
    tree.drag.settle = DRAG_SETTLE;

    let at = end.pointer.position * window.scale_factor();
    let rects = row_rects(&layout);
    let spot = drop_spot(&state.doc, &dragged, &rects, host_rect(&hosts), at);
    let parent = match spot {
        DropSpot::Row(id) => Some(id),
        DropSpot::Root => None,
        DropSpot::Invalid | DropSpot::Outside => return,
    };
    if dragged
        .iter()
        .all(|id| state.doc.get(*id).is_some_and(|obj| obj.parent == parent))
    {
        return;
    }
    undo.checkpoint(&state, "Reparent");
    for id in &dragged {
        state.reparent(*id, parent);
    }
    if let Some(parent) = parent {
        tree.expand(parent);
    }
}

/// Right click keeps an existing multi-selection and opens the object menu for it.
fn open_row_menu(
    mut click: On<PointerClick>,
    rows: Query<&SceneRow>,
    mut menu: ResMut<MenuState>,
    mut state: ResMut<SceneState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if !state.is_selected(row.0) {
        state.select_only(row.0);
    }
    menu::open(&mut menu, click.pointer.position, state.primary());
}

/// Right click on the tree background opens the menu for the selection.
fn open_tree_menu(
    mut click: On<PointerClick>,
    mut menu: ResMut<MenuState>,
    state: Res<SceneState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    click.propagate(false);
    menu::open(&mut menu, click.pointer.position, state.primary());
}

/// Console panel body: the log lines, without a tab strip.
pub fn console() -> Part {
    let banner = format!(
        "{} Engine v0.20.0-dev (c) 2026 {} Contributors.",
        crate::PRODUCT,
        crate::PRODUCT
    );
    let mut lines = Vec::new();
    for line in [
        banner.as_str(),
        "Vulkan 1.3.280 - Clustered Forward - Device #0",
        "Loaded project: project://project.ember",
    ] {
        lines.push(text_mono(line, FONT_SIZE_OUTPUT, tokens::TEXT));
    }
    lines.push(Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(2),
        }
        crate::console::ConsoleRows
    }) as Part);

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect {
                left: px(8),
                right: px(8),
                top: px(6),
            },
            row_gap: px(2),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::INSET)
        Children [ {lines} ]
    })
}

/// Host for the undo history rows.
#[derive(Component, Clone, Default)]
pub struct HistoryRows;

/// Rebuilds the history list when the undo stack changes, newest entry first.
pub(crate) fn sync_history(
    undo: &Undo,
    hosts: &Query<Entity, With<HistoryRows>>,
    commands: &mut Commands,
    last: &mut Option<Vec<&'static str>>,
) {
    let names: Vec<&'static str> = undo.names().collect();
    if last.as_ref() == Some(&names) {
        return;
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    commands.entity(host).despawn_children();

    if names.is_empty() {
        let empty = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
            }
            ChildOf(host)
            Children [ {text("No history", FONT_SIZE, tokens::TEXT_DIM)} ]
        };
        commands.spawn_scene(empty);
        return;
    }

    for (i, label) in names.iter().enumerate() {
        let (bg, fg) = row_colors(i == 0);
        let row = bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                column_gap: px(6),
            }
            ThemeBackgroundColor(bg)
            ChildOf(host)
            Children [ {text(*label, FONT_SIZE, fg)} ]
        };
        commands.spawn_scene(row);
    }
    *last = Some(names);
}

fn dock_frame(tabs: Vec<Part>, tail: Vec<Part>, body: Part, strip: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            {tab_strip(tabs, tail, strip)}
            --
            {body}
        ]
    })
}

/// History panel body: its own `Node`/`History` tab set and the undo rows.
pub fn history() -> Part {
    let tabs = vec![tab("Node", false), tab("History", true)];
    let tail = vec![
        icon_button(icons::close(), 18.0),
        icon_button(icons::dock_menu(), 20.0),
    ];

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { top: px(3) },
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
            }
            HistoryRows
        ]
    });

    dock_frame(tabs, tail, body, tokens::WINDOW)
}

/// Frame times kept for the Stats history graph.
pub const STATS_HISTORY: usize = 60;

/// Live editor statistics sampled a few times per second.
#[derive(Resource, Clone, Default)]
pub struct EditorStats {
    /// Frames per second, rounded.
    pub fps: u32,
    /// Last frame time in milliseconds.
    pub frame_ms: f32,
    /// Live entity count.
    pub entities: u32,
    /// Recent frame times in milliseconds, oldest first.
    pub history: Vec<f32>,
    /// Fastest frame in the history window.
    pub min_ms: f32,
    /// Mean frame time in the history window.
    pub avg_ms: f32,
    /// Slowest frame in the history window.
    pub max_ms: f32,
    /// Bumped after each sample.
    pub rev: u64,
}

impl EditorStats {
    /// Records one frame time and refreshes the summary.
    pub fn sample(&mut self, frame_ms: f32, entities: u32) {
        let frame_ms = frame_ms.max(0.000_1);
        self.fps = (1000.0 / frame_ms).round() as u32;
        self.frame_ms = frame_ms;
        self.entities = entities;
        if self.history.len() == STATS_HISTORY {
            self.history.remove(0);
        }
        self.history.push(frame_ms);
        self.min_ms = self.history.iter().copied().fold(f32::INFINITY, f32::min);
        self.max_ms = self.history.iter().copied().fold(0.0, f32::max);
        self.avg_ms = self.history.iter().sum::<f32>() / self.history.len() as f32;
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Host for the Stats rows.
#[derive(Component, Clone, Copy, Default)]
pub struct StatsRows;

/// Samples frame time and entity count without rebuilding every frame.
pub fn update_stats(
    time: Res<Time>,
    entities: &Entities,
    mut stats: ResMut<EditorStats>,
    mut elapsed: Local<f32>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.25 {
        return;
    }
    *elapsed = 0.0;
    let delta = time.delta_secs().max(0.000_001);
    stats.sample(delta * 1000.0, entities.len());
}

/// Stats dock body.
pub fn stats() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(2),
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                padding: UiRect::all(px(8)),
                row_gap: px(3),
            }
            StatsRows
        ]
    })
}

/// Rebuilds the Stats rows when a new sample arrives.
pub fn sync_stats(
    stats: Res<EditorStats>,
    hosts: Query<Entity, With<StatsRows>>,
    mut commands: Commands,
    mut shown: Local<Option<(Entity, u64)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    if shown.as_ref() == Some(&(host, stats.rev)) {
        return;
    }
    *shown = Some((host, stats.rev));
    commands.entity(host).despawn_children();
    for (label, value) in [
        ("FPS", format!("{}", stats.fps)),
        ("Frame", format!("{:.2} ms", stats.frame_ms)),
        ("Entities", format!("{}", stats.entities)),
    ] {
        let label = text(label, FONT_SIZE, tokens::TEXT_DIM);
        let value = text(value, FONT_SIZE, tokens::TEXT);
        commands.spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                height: px(22),
            }
            ChildOf(host)
            Children [ {label} -- {spacer()} -- {value} ]
        });
    }

    if stats.history.is_empty() {
        return;
    }

    let caption = text(
        format!("Frame history ({STATS_HISTORY})"),
        FONT_SIZE,
        tokens::TEXT_DIM,
    );
    commands.spawn_scene(bsn! {
        Node { height: px(20), display: Display::Flex, align_items: AlignItems::Center }
        ChildOf(host)
        Children [ {caption} ]
    });

    let graph = commands
        .spawn_scene(bsn! {
            Node {
                height: px(48),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexEnd,
                column_gap: px(1),
                padding: UiRect { left: px(4), right: px(4) },
            }
            ChildOf(host)
        })
        .id();
    let max = stats.max_ms.max(1.0);
    for frame in &stats.history {
        let height = (frame / max * 44.0).max(1.0);
        let color = if *frame > 16.7 {
            tokens::AXIS_X
        } else {
            tokens::ACCENT
        };
        commands.spawn_scene(bsn! {
            Node { width: px(3), height: px(height), border_radius: px(1) }
            ThemeBackgroundColor(color)
            ChildOf(graph)
        });
    }

    for (label, value) in [
        ("Min", format!("{:.2} ms", stats.min_ms)),
        ("Avg", format!("{:.2} ms", stats.avg_ms)),
        ("Max", format!("{:.2} ms", stats.max_ms)),
    ] {
        let label = text(label, FONT_SIZE, tokens::TEXT_DIM);
        let value = text(value, FONT_SIZE, tokens::TEXT);
        commands.spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                height: px(22),
            }
            ChildOf(host)
            Children [ {label} -- {spacer()} -- {value} ]
        });
    }
}

/// Host for the Outline rows.
#[derive(Component, Clone, Copy, Default)]
pub struct OutlineRows;

/// A row that selects one object.
#[derive(Component, Clone, Copy, Default)]
pub struct OutlineRow(pub u64);

/// Outline dock body.
pub fn outline() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::PANEL)
        Children [
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::scroll_y(),
            }
            ScrollArea
            OutlineRows
        ]
    })
}

/// Rebuilds the Outline from the document and selection.
pub fn sync_outline(
    state: Res<SceneState>,
    hosts: Query<Entity, With<OutlineRows>>,
    mut commands: Commands,
    mut shown: Local<Option<(Entity, u64, u64)>>,
) {
    let Ok(host) = hosts.single() else {
        return;
    };
    let selection: Vec<u64> = state.selection.items.clone();
    let signature = (
        host,
        state.rev,
        selection.iter().fold(0u64, |acc, id| acc.wrapping_add(*id)),
    );
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    commands.entity(host).despawn_children();

    let mut counts: alloc::collections::BTreeMap<&'static str, usize> = Default::default();
    for obj in &state.doc.objects {
        *counts.entry(obj.kind.label()).or_default() += 1;
    }
    let heading = text(
        format!("{} objects", state.doc.len()),
        FONT_SIZE,
        tokens::TEXT_DIM,
    );
    commands.spawn_scene(bsn! {
        Node { padding: UiRect::horizontal(px(8)), height: px(20), display: Display::Flex, align_items: AlignItems::Center }
        ChildOf(host)
        Children [ {heading} ]
    });
    for (kind, count) in counts {
        let label = text(format!("{kind}  x{count}"), FONT_SIZE, tokens::TEXT);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)), height: px(20), display: Display::Flex, align_items: AlignItems::Center }
            ChildOf(host)
            Children [ {label} ]
        });
    }

    let heading = text("Selection", FONT_SIZE, tokens::TEXT_DIM);
    commands.spawn_scene(bsn! {
        Node {
            padding: UiRect { left: px(8), right: px(8), top: px(4), bottom: px(2) },
            height: px(22),
            display: Display::Flex,
            align_items: AlignItems::Center,
            border: UiRect { top: px(1) },
        }
        ThemeBorderColor(tokens::BORDER)
        ChildOf(host)
        Children [ {heading} ]
    });
    if selection.is_empty() {
        let empty = text("No selection", FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)), height: px(20), display: Display::Flex, align_items: AlignItems::Center }
            ChildOf(host)
            Children [ {empty} ]
        });
        return;
    }
    for id in selection {
        let Some(obj) = state.doc.get(id) else {
            continue;
        };
        let label = text(
            format!("{}  {}", obj.kind.label(), obj.name),
            FONT_SIZE,
            tokens::TEXT,
        );
        commands
            .spawn_scene(bsn! {
                Node {
                    padding: UiRect::horizontal(px(8)),
                    height: px(22),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                }
                Button
                OutlineRow(id)
                ChildOf(host)
                Children [ {label} ]
            })
            .observe(select_outline_row);
    }
}

fn select_outline_row(
    activate: On<Activate>,
    rows: Query<&OutlineRow>,
    mut state: ResMut<SceneState>,
) {
    let Ok(row) = rows.get(activate.entity) else {
        return;
    };
    state.select_only(row.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::ObjectTransform;

    #[test]
    fn stats_history_keeps_the_last_window_and_summarises_it() {
        let mut stats = EditorStats::default();
        for frame in 1..=70 {
            stats.sample(frame as f32, 3);
        }
        assert_eq!(stats.history.len(), STATS_HISTORY);
        assert_eq!(stats.history[0], 11.0);
        assert_eq!(*stats.history.last().unwrap(), 70.0);
        assert_eq!(stats.min_ms, 11.0);
        assert_eq!(stats.max_ms, 70.0);
        assert!((stats.avg_ms - 40.5).abs() < 0.001);
        assert_eq!(stats.entities, 3);
    }

    #[test]
    fn row_colors_follow_the_selection() {
        assert!(row_colors(true) == (tokens::SELECT, tokens::SELECT_TEXT));
        assert!(row_colors(false) == (tokens::CLEAR, tokens::TEXT));
    }

    #[test]
    fn clicks_read_the_modifiers() {
        assert!(row_click(false, false) == RowClick::Replace);
        assert!(row_click(true, false) == RowClick::Extend);
        assert!(row_click(false, true) == RowClick::Toggle);
        assert!(row_click(true, true) == RowClick::Extend);
    }

    fn doc_with_tree() -> SceneDoc {
        let mut doc = SceneDoc::new();
        let a = doc.add(ObjectKind::Empty, "A", ObjectTransform::default());
        let b = doc.add(ObjectKind::Cube, "B", ObjectTransform::default());
        let c = doc.add(ObjectKind::Cube, "C", ObjectTransform::default());
        let d = doc.add(ObjectKind::Empty, "D", ObjectTransform::default());
        let e = doc.add(ObjectKind::Sphere, "E", ObjectTransform::default());
        assert!(doc.set_parent(b, Some(a)));
        assert!(doc.set_parent(c, Some(a)));
        assert!(doc.set_parent(e, Some(b)));
        let _ = d;
        doc
    }

    #[test]
    fn rows_follow_the_tree_in_document_order() {
        let doc = doc_with_tree();
        let want = vec![(1, 0), (2, 1), (5, 2), (3, 1), (4, 0)];
        assert_eq!(tree_order(&doc, &HashSet::new()), want);
    }

    #[test]
    fn a_flat_document_renders_in_document_order() {
        let mut doc = SceneDoc::new();
        let ids: Vec<u64> = (0..4)
            .map(|i| {
                doc.add(
                    ObjectKind::Cube,
                    format!("Cube {i}"),
                    ObjectTransform::default(),
                )
            })
            .collect();
        let want: Vec<(u64, usize)> = ids.iter().map(|id| (*id, 0)).collect();
        assert_eq!(tree_order(&doc, &HashSet::new()), want);
    }

    #[test]
    fn collapsing_hides_the_whole_subtree() {
        let doc = doc_with_tree();

        let collapsed: HashSet<u64> = [1].into_iter().collect();
        assert_eq!(tree_order(&doc, &collapsed), vec![(1, 0), (4, 0)]);

        let collapsed: HashSet<u64> = [2].into_iter().collect();
        assert_eq!(
            tree_order(&doc, &collapsed),
            vec![(1, 0), (2, 1), (3, 1), (4, 0)],
            "collapsing a middle node hides only its descendants"
        );
    }

    #[test]
    fn drops_reject_self_descendants_and_dragged_members() {
        let doc = doc_with_tree();

        assert!(can_drop(&doc, &[2], 4), "moving to another root is legal");
        assert!(can_drop(&doc, &[5], 1), "reparenting up the tree is legal");
        assert!(!can_drop(&doc, &[1], 1), "self drop slipped through");
        assert!(
            !can_drop(&doc, &[1], 2),
            "parent onto its own child slipped through"
        );
        assert!(!can_drop(&doc, &[1], 5), "descendant drop slipped through");
        assert!(
            !can_drop(&doc, &[2, 5], 5),
            "multi-drag onto a member slipped through"
        );
        assert!(!can_drop(&doc, &[1], 99), "unknown target slipped through");
    }

    #[test]
    fn drop_spots_pick_rows_then_the_tree_body() {
        let doc = doc_with_tree();
        let row = Rect::from_center_size(Vec2::new(100.0, 50.0), Vec2::new(200.0, 20.0));
        let body = Rect::from_center_size(Vec2::new(100.0, 200.0), Vec2::new(200.0, 400.0));
        let rows = [(2u64, row)];

        assert_eq!(
            drop_spot(&doc, &[4], &rows, Some(body), Vec2::new(100.0, 50.0)),
            DropSpot::Row(2)
        );
        assert_eq!(
            drop_spot(&doc, &[2], &rows, Some(body), Vec2::new(100.0, 50.0)),
            DropSpot::Invalid
        );
        assert_eq!(
            drop_spot(&doc, &[4], &rows, Some(body), Vec2::new(100.0, 300.0)),
            DropSpot::Root
        );
        assert_eq!(
            drop_spot(&doc, &[4], &rows, None, Vec2::new(100.0, 300.0)),
            DropSpot::Outside,
            "outside the tree cancels the drop"
        );
    }

    #[test]
    fn dragging_a_selected_row_carries_the_selection() {
        let mut state = SceneState::default();
        let a = state.spawn(ObjectKind::Cube);
        let b = state.spawn(ObjectKind::Cube);
        let c = state.spawn(ObjectKind::Cube);
        state.select_many(&[a, c]);

        assert_eq!(drag_set(&state, a), vec![a, c]);
        assert_eq!(drag_set(&state, b), vec![b]);
    }

    #[test]
    fn a_filter_keeps_matches_and_their_ancestors() {
        let doc = doc_with_tree();
        let filter = SceneFilter {
            query: "E".into(),
            rev: 1,
        };
        let visible = filter.visible(&doc).unwrap();
        assert!(visible.contains(&5), "matching object");
        assert!(visible.contains(&2), "parent of the match");
        assert!(visible.contains(&1), "root ancestor");
        assert!(!visible.contains(&3), "unrelated sibling");
        assert!(!visible.contains(&4), "unrelated root");
    }

    #[test]
    fn an_empty_filter_shows_everything() {
        let doc = doc_with_tree();
        assert!(SceneFilter::default().visible(&doc).is_none());
        assert!(
            SceneFilter {
                query: "   ".into(),
                rev: 1,
            }
            .visible(&doc)
            .is_none()
        );
    }

    #[test]
    fn the_filter_is_case_insensitive() {
        let doc = doc_with_tree();
        let visible = SceneFilter {
            query: "b".into(),
            rev: 1,
        }
        .visible(&doc)
        .unwrap();
        assert!(visible.contains(&2));
    }

    #[test]
    fn the_filter_field_updates_the_resource() {
        use eme::ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.insert_resource(SceneFilter::default());
        world.spawn((EditableText::new("cube"), SceneFilterInput));
        world.run_system_once(sync_scene_filter).unwrap();
        let filter = world.resource::<SceneFilter>();
        assert_eq!(filter.query, "cube");
        assert_eq!(filter.rev, 1);
    }

    #[test]
    fn signature_tracks_the_document_not_the_selection() {
        let mut state = SceneState::default();
        let a = state.spawn(ObjectKind::Cube);
        let b = state.spawn(ObjectKind::Sphere);
        let mut tree = SceneTree::default();
        let none = HashMap::new();
        let node = NodeSelection::default();
        let sig = tree_signature(&state, &tree, &none, &node, &SceneFilter::default());

        state.select_only(a);
        assert_eq!(tree_signature(&state, &tree, &none, &node, &SceneFilter::default()), sig);

        tree.toggle(a);
        let collapsed = tree_signature(&state, &tree, &none, &node, &SceneFilter::default());
        assert_ne!(collapsed, sig, "collapse must force a rebuild");

        state.doc.rename(a, "Crate");
        let renamed = tree_signature(&state, &tree, &none, &node, &SceneFilter::default());
        assert_ne!(renamed, collapsed);

        state.doc.set_parent(a, Some(b));
        assert_ne!(tree_signature(&state, &tree, &none, &node, &SceneFilter::default()), renamed);
    }
}
