//! The `FileSystem` dock: a live tree over the project folder.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::prelude::*;
use eme::window::{FileDragAndDrop, PrimaryWindow, Window};
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button, ScrollArea};

use super::chrome::*;
use super::icons;
use crate::build;
use crate::console::ConsoleLog;
use crate::import::{ImportRequest, ImportService, OptionsPanel};
use crate::persist;
use crate::play::PlayState;
use crate::scene::SceneState;
use crate::tokens;
use crate::undo::Undo;

/// Deepest directory level listed below the project root.
const MAX_DEPTH: usize = 4;
/// Entries kept per directory before the truncation row.
const MAX_ENTRIES: usize = 150;
/// Left padding added for each level of nesting.
const INDENT: f32 = 12.0;
/// Square reserved for a row's disclosure triangle.
const CARET: f32 = 12.0;
/// Directory names that never hold authoring content.
const SKIP: &[&str] = &["target", "build", "tests-integration"];

/// What a row's name says about the entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Dir,
    Scene,
    File,
}

fn kind(name: &str, is_dir: bool) -> Kind {
    if is_dir {
        return Kind::Dir;
    }
    match Path::new(name).extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("ember") => Kind::Scene,
        _ => Kind::File,
    }
}

/// The `FileSystem` dock's tree state: the last scan, the open folders, and the
/// selected file.
///
/// Rows are rebuilt only when the scan's signature or the view state changes, so
/// a folder toggle or a selection does not touch the disk.
#[derive(Resource)]
pub struct FileTree {
    root: PathBuf,
    scan: Scan,
    expanded: HashSet<String>,
    selected: Option<String>,
    signature: Option<u64>,
    rev: u64,
    built: u64,
    host: Option<Entity>,
    ready: bool,
    /// A rebuild was requested while a row was being clicked; it runs next frame.
    pending: bool,
}

impl Default for FileTree {
    fn default() -> Self {
        let mut tree = Self::new(build::workspace_dir());
        tree.expanded.insert("scenes".into());
        tree
    }
}

impl FileTree {
    /// The project folder this tree is rooted at.
    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    /// Relative path of the selected row, if any.
    pub(crate) fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    fn new(root: PathBuf) -> Self {
        Self {
            root,
            scan: Scan::default(),
            expanded: HashSet::new(),
            selected: None,
            signature: None,
            rev: 0,
            built: 0,
            host: None,
            ready: false,
            pending: false,
        }
    }

    /// Rescans the project folder, returning whether its signature changed.
    fn rescan(&mut self) -> bool {
        let scan = scan(&self.root, &self.expanded);
        if self.ready && self.signature == Some(scan.signature) {
            return false;
        }
        self.scan = scan;
        self.signature = Some(self.scan.signature);
        self.ready = true;
        self.rev = self.rev.wrapping_add(1);
        true
    }

    fn toggle(&mut self, rel: &str) {
        if !self.expanded.remove(rel) {
            self.expanded.insert(rel.to_string());
        }
        self.signature = None;
        self.rescan();
        self.rev = self.rev.wrapping_add(1);
    }

    fn select(&mut self, rel: String) {
        if self.selected.as_deref() == Some(rel.as_str()) {
            return;
        }
        self.selected = Some(rel);
        self.rev = self.rev.wrapping_add(1);
    }
}

/// One directory's visible entries plus how many were dropped by the cap.
#[derive(Clone, Default)]
struct Listing {
    nodes: Vec<FsNode>,
    more: usize,
}

/// One directory or file below the project root.
#[derive(Clone, Default)]
struct FsNode {
    name: String,
    rel: String,
    is_dir: bool,
    at_cap: bool,
    children: Listing,
}

/// A full walk of the project folder and the hash that identifies its contents.
#[derive(Clone, Default)]
struct Scan {
    root: Listing,
    missing: bool,
    signature: u64,
}

/// One line in the dock, in display order.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Row {
    Entry {
        name: String,
        rel: String,
        is_dir: bool,
        depth: usize,
    },
    More {
        count: usize,
        depth: usize,
    },
    Empty {
        depth: usize,
    },
    MissingRoot,
}

fn scan(root: &Path, expanded: &HashSet<String>) -> Scan {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if !root.is_dir() {
        return Scan {
            root: Listing::default(),
            missing: true,
            signature: 0,
        };
    }
    for rel in expanded {
        hasher.write(rel.as_bytes());
    }
    let listing = scan_dir(root, "", 1, &mut hasher, expanded);
    Scan {
        root: listing,
        missing: false,
        signature: hasher.finish(),
    }
}

fn scan_dir(
    dir: &Path,
    rel: &str,
    depth: usize,
    hasher: &mut std::collections::hash_map::DefaultHasher,
    expanded: &HashSet<String>,
) -> Listing {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Listing::default();
    };
    let mut entries: Vec<(String, bool, Option<(u64, u128)>)> = Vec::new();
    for entry in read.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        if name.ends_with(".import.ron") || name.ends_with(".import-preset.ron") {
            continue;
        }
        let meta = entry.metadata().ok().map(|meta| (meta.len(), mtime(&meta)));
        entries.push((name, file_type.is_dir(), meta));
    }
    entries.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.0.cmp(&b.0))
    });

    let (dirs, files): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| entry.1);
    let total = dirs.len() + files.len();
    for entry in dirs.iter().chain(files.iter()) {
        entry.0.hash(hasher);
        entry.1.hash(hasher);
        if let Some((len, modified)) = entry.2 {
            len.hash(hasher);
            modified.hash(hasher);
        }
    }

    let listed: Vec<_> = dirs.into_iter().chain(files).take(MAX_ENTRIES).collect();
    let mut listing = Listing {
        nodes: Vec::with_capacity(listed.len()),
        more: total.saturating_sub(listed.len()),
    };
    for (name, is_dir, _) in listed {
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let children = if is_dir && depth < MAX_DEPTH && expanded.contains(&child_rel) {
            scan_dir(&dir.join(&name), &child_rel, depth + 1, hasher, expanded)
        } else {
            Listing::default()
        };
        listing.nodes.push(FsNode {
            name,
            rel: child_rel,
            is_dir,
            at_cap: is_dir && depth >= MAX_DEPTH,
            children,
        });
    }
    listing
}

fn mtime(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos())
}

fn rows(scan: &Scan, expanded: &HashSet<String>) -> Vec<Row> {
    if scan.missing {
        return vec![Row::MissingRoot];
    }
    let mut out = Vec::new();
    flatten(&scan.root, expanded, 0, &mut out);
    if out.is_empty() {
        out.push(Row::Empty { depth: 0 });
    }
    out
}

fn flatten(listing: &Listing, expanded: &HashSet<String>, depth: usize, out: &mut Vec<Row>) {
    for node in &listing.nodes {
        out.push(Row::Entry {
            name: node.name.clone(),
            rel: node.rel.clone(),
            is_dir: node.is_dir,
            depth,
        });
        if !node.is_dir || node.at_cap || !expanded.contains(&node.rel) {
            continue;
        }
        if node.children.nodes.is_empty() {
            out.push(Row::Empty { depth: depth + 1 });
            continue;
        }
        flatten(&node.children, expanded, depth + 1, out);
        if node.children.more > 0 {
            out.push(Row::More {
                count: node.children.more,
                depth: depth + 1,
            });
        }
    }
    if listing.more > 0 {
        out.push(Row::More {
            count: listing.more,
            depth,
        });
    }
}

/// Host for the file rows; its children are rebuilt when the scan changes.
#[derive(Component, Clone, Default)]
pub(crate) struct FileRows;

/// One row in the `FileSystem` dock, keyed by its path below the project root.
#[derive(Component, Clone, Default)]
pub(crate) struct FileRow {
    rel: String,
    is_dir: bool,
}

/// Project panel body: the breadcrumb and the scrolling file tree.
pub fn body() -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect { bottom: px(4) },
        }
        ThemeBackgroundColor(tokens::INSET)
        Children [
            {breadcrumb()}
            --
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::scroll_y(),
            }
            ScrollArea
            Children [ {file_rows()} ]
        ]
    })
}

fn breadcrumb() -> Part {
    Box::new(bsn! {
        Node {
            height: px(24),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(3),
        }
        Children [
            {icon_button(icons::folder(), 16.0)}
            --
            {text("project://", FONT_SIZE, tokens::TEXT_MEDIUM)}
            --
            {icon_button(icons::breadcrumb_caret(), 12.0)}
        ]
    })
}

fn file_rows() -> Part {
    Box::new(bsn! {
        Node {
            width: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        FileRows
    })
}

pub(crate) fn refresh_button() -> Part {
    Box::new(bsn! {
        Node {
            width: px(20),
            height: px(20),
            align_self: AlignSelf::Center,
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Button
        on(refresh_files)
        Children [ {icons::dock_menu()} ]
    })
}

fn refresh_files(_click: On<Activate>, mut tree: ResMut<FileTree>) {
    tree.rescan();
}

/// Rebuilds the dock's rows when the scan or the view state changed.
pub fn sync_file_rows(
    mut commands: Commands,
    hosts: Query<Entity, With<FileRows>>,
    tree: Option<ResMut<FileTree>>,
) {
    let Some(mut tree) = tree else {
        commands.init_resource::<FileTree>();
        return;
    };
    if !tree.ready {
        tree.rescan();
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    if tree.host == Some(host) && tree.built == tree.rev && !tree.pending {
        return;
    }
    if !tree.pending {
        tree.host = Some(host);
        tree.pending = true;
        return;
    }
    tree.pending = false;
    tree.host = Some(host);
    tree.built = tree.rev;
    commands.entity(host).despawn_children();
    for row in rows(&tree.scan, &tree.expanded) {
        spawn_row(&mut commands, host, &tree, row);
    }
}

/// Rescans the project folder; run once a second so edits on disk show up.
pub fn poll_files(mut tree: ResMut<FileTree>) {
    tree.rescan();
}

fn spawn_row(commands: &mut Commands, host: Entity, tree: &FileTree, row: Row) {
    match row {
        Row::Entry {
            name,
            rel,
            is_dir,
            depth,
        } => spawn_entry(commands, host, tree, name, rel, is_dir, depth),
        Row::More { count, depth } => {
            spawn_muted(commands, host, format!("… {count} more"), depth);
        }
        Row::Empty { depth } => spawn_muted(commands, host, "empty".into(), depth),
        Row::MissingRoot => {
            spawn_muted(commands, host, "no project folder".into(), 0);
        }
    }
}

fn spawn_entry(
    commands: &mut Commands,
    host: Entity,
    tree: &FileTree,
    name: String,
    rel: String,
    is_dir: bool,
    depth: usize,
) {
    let open = tree.expanded.contains(&rel);
    let selected = !is_dir && tree.selected.as_deref() == Some(rel.as_str());
    let (bg, fg) = row_colors(selected);
    let glyph = match kind(&name, is_dir) {
        Kind::Dir => icons::folder(),
        Kind::Scene => icons::badge(icons::SCENE),
        Kind::File => icons::file(),
    };

    let mut kids: Vec<Part> = Vec::new();
    if is_dir {
        kids.push(Box::new(bsn! {
            Node {
                width: px(CARET),
                height: px(CARET),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            Children [ {icons::caret_tinted(open, tokens::ICON)} ]
        }));
    } else {
        kids.push(Box::new(bsn! {
            Node {
                width: px(CARET),
                height: px(CARET),
            }
        }));
    }
    kids.push(glyph);
    kids.push(text(name, FONT_SIZE, fg));

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
        FileRow { rel, is_dir }
        ChildOf(host)
        Children [ {kids} ]
        on(click_file)
        on(open_folder_menu)
        on(start_asset_drag)
    };
    commands.spawn_scene(row);
}

fn spawn_muted(commands: &mut Commands, host: Entity, label: String, depth: usize) {
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
        }
        ChildOf(host)
        Children [ {text(label, FONT_SIZE, tokens::TEXT_DIM)} ]
    };
    commands.spawn_scene(row);
}

fn row_colors(selected: bool) -> (ThemeToken, ThemeToken) {
    if selected {
        (tokens::SELECT, tokens::SELECT_TEXT)
    } else {
        (tokens::CLEAR, tokens::TEXT)
    }
}

/// Opens folders, loads `.ember` scenes, and selects every other file.
fn click_file(
    click: On<Activate>,
    rows: Query<&FileRow>,
    mut tree: ResMut<FileTree>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut tabs: ResMut<crate::screens::editor::tabs::Tabs>,
    play: Res<PlayState>,
) {
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    let rel = row.rel.clone();
    if row.is_dir {
        tree.toggle(&rel);
        return;
    }
    if kind(&rel, false) != Kind::Scene {
        tree.select(rel);
        return;
    }
    if play.running() {
        warn!("not opening {rel}: a play session is running");
        return;
    }
    let path = tree.root.join(&rel);
    match persist::load_from(&path.to_string_lossy()) {
        Ok(Some(doc)) => {
            let count = doc.len();
            state.replace_doc(doc);
            undo.clear();
            tree.select(rel.clone());
            if let Some(name) = Path::new(&rel).file_name() {
                tabs.set_scene_label(name.to_string_lossy().into_owned());
            }
            info!("opened {rel} ({count} objects)");
        }
        Ok(None) => warn!("no scene at {rel}"),
        Err(err) => error!("could not open {rel}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_project_row_becomes_assets_relative() {
        let root = Path::new("/project");
        let assets = Path::new("/project/assets");
        assert_eq!(
            assets_relative_in(root, assets, "assets/models/chair.glb").as_deref(),
            Some("models/chair.glb")
        );
        assert_eq!(assets_relative_in(root, assets, "scenes/main.ember"), None);
    }

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("eme-files-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn dir(&self, rel: &str) {
            std::fs::create_dir_all(self.0.join(rel)).unwrap();
        }

        fn file(&self, rel: &str, body: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                Row::Entry { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn open(rels: &[&str]) -> HashSet<String> {
        rels.iter().map(|rel| (*rel).to_string()).collect()
    }

    #[test]
    fn listing_puts_dirs_first_and_skips_noise() {
        let tmp = Tmp::new("order");
        tmp.dir("zdir");
        tmp.dir("adir");
        tmp.dir(".hidden");
        tmp.dir("target");
        tmp.dir("build");
        tmp.dir("tests-integration");
        tmp.file("b.rs", "fn main() {}");
        tmp.file("a.png", "");
        tmp.file(".secret", "");

        let scan = scan(&tmp.0, &HashSet::new());
        let rows = rows(&scan, &HashSet::new());
        assert_eq!(names(&rows), ["adir", "zdir", "a.png", "b.rs"]);
        assert!(rows.iter().all(|row| matches!(row, Row::Entry { .. })));
    }

    #[test]
    fn recursion_stops_at_the_depth_cap() {
        let tmp = Tmp::new("depth");
        tmp.dir("d1/d2/d3/d4/d5/d6");

        let expanded = open(&["d1", "d1/d2", "d1/d2/d3", "d1/d2/d3/d4", "d1/d2/d3/d4/d5"]);
        let scan = scan(&tmp.0, &expanded);
        let rows = rows(&scan, &expanded);

        assert_eq!(names(&rows), ["d1", "d2", "d3", "d4"]);
        assert_eq!(
            rows.last(),
            Some(&Row::Entry {
                name: "d4".into(),
                rel: "d1/d2/d3/d4".into(),
                is_dir: true,
                depth: 3,
            })
        );
    }

    #[test]
    fn expanding_and_collapsing_change_the_visible_rows() {
        let tmp = Tmp::new("toggle");
        tmp.file("dir/a.rs", "fn main() {}");
        tmp.dir("empty");

        let scan = scan(&tmp.0, &open(&["dir", "empty"]));
        assert_eq!(names(&rows(&scan, &HashSet::new())), ["dir", "empty"]);
        assert_eq!(
            names(&rows(&scan, &open(&["dir", "empty"]))),
            ["dir", "a.rs", "empty"]
        );
        assert!(
            rows(&scan, &open(&["dir", "empty"])).contains(&Row::Empty { depth: 1 }),
            "an expanded empty folder needs a placeholder row"
        );
    }

    #[test]
    fn a_directory_cap_ends_in_a_truncation_row() {
        let tmp = Tmp::new("cap");
        for i in 0..160 {
            tmp.file(&format!("many/{i:03}.rs"), "");
        }

        let expanded = open(&["many"]);
        let scan = scan(&tmp.0, &expanded);
        let rows = rows(&scan, &expanded);
        let entries = names(&rows);
        assert_eq!(entries.len(), MAX_ENTRIES + 1);
        assert_eq!(entries[0], "many");
        assert!(rows.contains(&Row::More {
            count: 10,
            depth: 1,
        }));
    }

    #[test]
    fn ember_files_are_scenes_and_other_files_are_plain() {
        assert_eq!(kind("main.ember", false), Kind::Scene);
        assert_eq!(kind("MAIN.EMBER", false), Kind::Scene);
        assert_eq!(kind("main.rs", false), Kind::File);
        assert_eq!(kind("tile.png", false), Kind::File);
        assert_eq!(kind("scenes", true), Kind::Dir);
    }

    #[test]
    fn signature_moves_for_a_new_file_and_holds_for_an_idle_scan() {
        let tmp = Tmp::new("sig");
        tmp.file("a.rs", "fn main() {}");

        let first = scan(&tmp.0, &HashSet::new()).signature;
        assert_eq!(scan(&tmp.0, &HashSet::new()).signature, first, "an idle scan must not churn");

        tmp.file("b.rs", "fn b() {}");
        assert_ne!(scan(&tmp.0, &HashSet::new()).signature, first);
    }

    #[test]
    fn a_selection_survives_a_rescan() {
        let tmp = Tmp::new("select");
        tmp.file("a.rs", "fn main() {}");

        let mut tree = FileTree::new(tmp.0.clone());
        tree.rescan();
        tree.select("a.rs".into());
        let rev = tree.rev;

        tree.rescan();
        assert_eq!(tree.selected.as_deref(), Some("a.rs"));
        assert_eq!(tree.rev, rev, "an unchanged scan must not force a rebuild");
    }

    #[test]
    fn a_missing_root_gets_one_explanatory_row() {
        let tmp = Tmp::new("missing");
        let scan = scan(&tmp.0.join("absent"), &HashSet::new());
        assert!(scan.missing);
        assert_eq!(rows(&scan, &HashSet::new()), [Row::MissingRoot]);
    }
}

/// Project-relative row path to the path the `AssetServer` and scene document use.
fn assets_relative(tree: &FileTree, rel: &str) -> Option<String> {
    assets_relative_in(&tree.root, &crate::asset_root(), rel)
}

fn assets_relative_in(root: &Path, assets: &Path, rel: &str) -> Option<String> {
    let absolute = root.join(rel);
    let rel = absolute.strip_prefix(assets).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Instances models that just finished importing, when the option is on.
pub fn instance_imports(
    mut service: ResMut<ImportService>,
    mut state: ResMut<SceneState>,
    mut log: ResMut<ConsoleLog>,
) {
    if service.finished.is_empty() {
        return;
    }
    let wanted = service.settings.output.instance_after_import;
    let forced = std::mem::take(&mut service.instantiate);
    let assets = crate::asset_root();
    for path in std::mem::take(&mut service.finished) {
        if !wanted && !forced.contains(&path) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(&assets) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if state.spawn_model(&rel, [0.0, 0.5, 0.0]).is_some() {
            log.push(format!("instanced {rel}"));
        }
    }
}

/// A model being dragged out of the tree towards the viewport.
#[derive(Resource, Default)]
pub struct AssetDrag(pub Option<String>);

pub fn start_asset_drag(
    start: On<PointerDragStart>,
    rows: Query<&FileRow>,
    parents: Query<&ChildOf>,
    mut drag: ResMut<AssetDrag>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let mut entity = start.entity;
    let row = loop {
        if let Ok(row) = rows.get(entity) {
            break row;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if row.is_dir || !is_model(Path::new(&row.rel)) {
        return;
    }
    drag.0 = Some(row.rel.clone());
}

pub fn drop_asset(
    mouse: Res<ButtonInput<MouseButton>>,
    mut drag: ResMut<AssetDrag>,
    window: Single<&Window, With<PrimaryWindow>>,
    viewport: Query<(&ComputedNode, &UiGlobalTransform), With<super::viewport::ViewportPanel>>,
    tree: Query<(&ComputedNode, &UiGlobalTransform), With<super::docks::SceneTreePanel>>,
    file_tree: Res<FileTree>,
    mut state: ResMut<SceneState>,
    mut log: ResMut<ConsoleLog>,
) {
    let Some(rel) = drag.0.clone() else {
        return;
    };
    if mouse.pressed(MouseButton::Left) {
        return;
    }
    drag.0 = None;

    let inside = |panel: &ComputedNode, global: &UiGlobalTransform| {
        let rect = Rect::from_center_size(global.translation.trunc(), panel.size());
        window
            .cursor_position()
            .map(|pos| pos * window.scale_factor())
            .is_some_and(|pos| rect.contains(pos))
    };
    let in_viewport = viewport
        .single()
        .is_ok_and(|(panel, global)| inside(panel, global));
    let in_tree = tree
        .single()
        .is_ok_and(|(panel, global)| inside(panel, global));
    if !in_viewport && !in_tree {
        log.push(format!("dropped {rel} outside the viewport and scene tree"));
        return;
    }
    let Some(asset_rel) = assets_relative(&file_tree, &rel) else {
        log.push(format!(
            "{rel} is outside assets/; import it into assets before dragging it into the scene"
        ));
        return;
    };
    match state.spawn_model(&asset_rel, [0.0, 0.0, 0.0]) {
        Some(_) if in_tree => log.push(format!("instanced {asset_rel} from the scene tree")),
        Some(_) => log.push(format!("instanced {asset_rel} at the origin")),
        None => log.push(format!("could not instance {asset_rel}")),
    }
}

/// The one-row import menu opened from a folder or the Project menu.
#[derive(Resource, Default)]
pub struct ImportMenu {
    /// Where to draw it, in window coordinates.
    pub at: Option<Vec2>,
    /// Folder it targets, relative to the project root.
    pub folder: Option<String>,
    /// Model files under `assets/` when the picker opened.
    pub choices: Vec<String>,
    /// Which page the menu shows.
    pub stage: ImportMenuStage,
}

/// Project menu pages: the entry row, then the model picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ImportMenuStage {
    /// A single Import row.
    #[default]
    Root,
    /// The model list.
    Models,
}

/// Host for the model rows rebuilt in the Project import menu.
#[derive(Component, Clone, Default)]
pub struct ImportMenuRows;

/// One model row in the Project import menu.
#[derive(Component, Clone, Default)]
pub struct ImportChoice(pub String);

/// The single entry row that opens the model picker.
#[derive(Component, Clone, Default)]
pub struct OpenModelPicker;

/// Opens the Project Settings window.
#[derive(Component, Clone, Default)]
pub struct OpenProjectSettings;

/// Marks the import menu's root node.
#[derive(Component, Clone, Copy, Default)]
pub struct ImportMenuRoot;

/// Opens the import menu for a folder row on secondary click.
pub fn open_folder_menu(
    click: On<PointerClick>,
    rows: Query<&FileRow>,
    mut menu: ResMut<ImportMenu>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Ok(row) = rows.get(click.entity) else {
        return;
    };
    if !row.is_dir {
        return;
    }
    menu.at = Some(click.pointer.position);
    menu.folder = Some(row.rel.clone());
}

/// Opens the same menu from the Project entry in the menu bar.
pub fn open_project_menu(
    _: On<Activate>,
    mut menu: ResMut<ImportMenu>,
    mut top: ResMut<super::menu::TopMenu>,
) {
    top.open = None;
    menu.at = Some(Vec2::new(8.0, 34.0));
    menu.folder = None;
    menu.stage = ImportMenuStage::Root;
    menu.choices.clear();
}

/// Switches the Project menu from its entry row to the model list.
fn open_model_picker(_: On<Activate>, mut menu: ResMut<ImportMenu>) {
    menu.stage = ImportMenuStage::Models;
    menu.choices = model_choices(&crate::asset_root());
}

/// Every model the project can import, relative to the project root.
fn model_choices(assets: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_models(assets, assets, 0, &mut out);
    out.sort_by_key(|path| (path.matches('/').count(), path.clone()));
    out.truncate(30);
    out
}

fn collect_models(assets: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 6 || out.len() >= 300 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_models(assets, &path, depth + 1, out);
            continue;
        }
        if is_model(&path)
            && let Ok(rel) = path.strip_prefix(assets)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Hides the import menu on Escape or when the screen goes away.
pub fn close_import_menu(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<ImportMenu>) {
    if keys.just_pressed(KeyCode::Escape) {
        menu.at = None;
    }
}

/// Draws the menu at its anchor and hides it when closed.
pub fn sync_import_menu(
    menu: Res<ImportMenu>,
    mut roots: Query<&mut Node, With<ImportMenuRoot>>,
    hosts: Query<Entity, With<ImportMenuRows>>,
    mut commands: Commands,
    mut shown: Local<Option<(Entity, Vec2, usize, ImportMenuStage)>>,
) {
    for mut node in &mut roots {
        let want = if menu.at.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
        if let Some(at) = menu.at {
            node.left = px(at.x);
            node.top = px(at.y);
        }
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let Some(at) = menu.at else {
        *shown = None;
        return;
    };
    let signature = (host, at, menu.choices.len(), menu.stage);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    commands.entity(host).despawn_children();
    if menu.stage == ImportMenuStage::Root {
        commands.spawn_scene(bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
            }
            Button
            OpenModelPicker
            ChildOf(host)
            Children [ {text("Import glTF…", FONT_SIZE, tokens::TEXT)} ]
            on(open_model_picker)
        });
        commands.spawn_scene(bsn! {
            Node {
                height: px(TREE_ROW_H),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
            }
            Button
            OpenProjectSettings
            ChildOf(host)
            Children [ {text("Project Settings…", FONT_SIZE, tokens::TEXT)} ]
            on(open_project_settings)
        });
        return;
    }
    if menu.choices.is_empty() {
        let hint = text(
            "no .glb/.gltf in the project; drop one in or import from FileSystem",
            FONT_SIZE,
            tokens::TEXT_DIM,
        );
        commands.spawn_scene(bsn! {
            Node {
                padding: UiRect::horizontal(px(8)),
                display: Display::Flex,
            }
            ChildOf(host)
            Children [ {hint} ]
        });
        return;
    }
    for choice in &menu.choices {
        let label = text_clipped(choice.clone(), FONT_SIZE, tokens::TEXT);
        let rel = choice.clone();
        commands
            .spawn_scene(bsn! {
                Node {
                    height: px(TREE_ROW_H),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(10)),
                    overflow: Overflow::clip(),
                }
                Button
                ThemeBackgroundColor(tokens::PANEL)
                ImportChoice(rel)
                ChildOf(host)
                Children [ {label} ]
                on(choose_import)
            });
    }
}

/// Opens the project settings window from the Project menu.
fn open_project_settings(
    _: On<Activate>,
    mut menu: ResMut<ImportMenu>,
    mut settings: ResMut<crate::settings::ProjectSettingsState>,
) {
    menu.at = None;
    crate::settings::open(&mut settings);
}

/// Opens import options for a model picked from the Project menu.
fn choose_import(
    activate: On<Activate>,
    rows: Query<&ImportChoice>,
    mut menu: ResMut<ImportMenu>,
    mut panel: ResMut<OptionsPanel>,
    mut log: ResMut<ConsoleLog>,
) {
    let Ok(choice) = rows.get(activate.entity) else {
        return;
    };
    let assets = crate::asset_root();
    let source = assets.join(&choice.0);
    if !is_model(&source) {
        log.push(format!("import: {} is not a .glb or .gltf file", choice.0));
        return;
    }
    let folder = source
        .parent()
        .and_then(|parent| parent.strip_prefix(&assets).ok())
        .map_or_else(|| PathBuf::from("assets"), |parent| PathBuf::from("assets").join(parent));
    menu.at = None;
    panel.open_for(&source, &folder);
    log.push(format!("opened import options for {}", choice.0));
}

/// The menu itself, a root-level overlay so it can overhang the dock.
pub fn import_menu() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(0),
            top: px(0),
            min_width: px(300),
            max_width: px(440),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(4),
            padding: px(2),
        }
        ImportMenuRoot
        GlobalZIndex(200)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                max_height: px(380),
                overflow: Overflow::scroll_y(),
            }
            ScrollArea
            ImportMenuRows
        ]
    })
}

pub fn is_model(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf"))
}

/// Copies dropped models into the hovered folder and imports them.
pub fn accept_drop(
    mut drops: MessageReader<FileDragAndDrop>,
    window: Single<&Window, With<PrimaryWindow>>,
    rows: Query<(&FileRow, &ComputedNode, &UiGlobalTransform)>,
    keys: Res<ButtonInput<KeyCode>>,
    tree: Res<FileTree>,
    mut service: ResMut<ImportService>,
    mut panel: ResMut<OptionsPanel>,
    mut log: ResMut<ConsoleLog>,
) {
    let mut dropped = Vec::new();
    for event in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            dropped.push(path_buf.clone());
        }
    }
    if dropped.is_empty() {
        return;
    }

    let scale = window.scale_factor();
    let cursor = window.cursor_position().map(|pos| pos * scale);
    let hovered = cursor.and_then(|pos| {
        rows.iter().find_map(|(row, node, global)| {
            let rect = Rect::from_center_size(global.translation.trunc(), node.size());
            rect.contains(pos).then(|| row.clone())
        })
    });

    let folder = match &hovered {
        Some(row) if row.is_dir => PathBuf::from(&row.rel),
        Some(row) => Path::new(&row.rel)
            .parent()
            .map_or_else(|| service.settings.output.folder.clone(), PathBuf::from),
        None => service.settings.output.folder.clone(),
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let over_file_panel = hovered.is_some();

    for source in dropped {
        if !is_model(&source) {
            log.push(format!(
                "import: {} is not a .glb or .gltf file",
                source.display()
            ));
            continue;
        }
        if shift {
            panel.open_for(&source, &folder);
            log.push(format!(
                "opened import options for {}",
                source.file_name().unwrap_or_default().to_string_lossy()
            ));
            continue;
        }
        let settings = service.settings.clone();
        let request = ImportRequest {
            source,
            folder: folder.clone(),
            settings,
            copy: true,
        };
        if over_file_panel {
            service.start(request, &tree.root, &mut log);
        } else {
            log.push("importing dropped file into the scene".to_string());
            service.start_in_scene(request, &tree.root, &mut log);
        }
    }
}
