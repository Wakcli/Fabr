//! The `FileSystem` dock: a live tree over the project folder.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use eme::feathers::theme::{ThemeBackgroundColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button, ScrollArea};

use super::chrome::*;
use super::icons;
use crate::build;
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
}

impl Default for FileTree {
    fn default() -> Self {
        let mut tree = Self::new(build::workspace_dir());
        tree.expanded.insert("scenes".into());
        tree
    }
}

impl FileTree {
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
        }
    }

    /// Rescans the project folder, returning whether its signature changed.
    fn rescan(&mut self) -> bool {
        let scan = scan(&self.root);
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

fn scan(root: &Path) -> Scan {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if !root.is_dir() {
        return Scan {
            root: Listing::default(),
            missing: true,
            signature: 0,
        };
    }
    let listing = scan_dir(root, "", 1, &mut hasher);
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
        let children = if is_dir && depth < MAX_DEPTH {
            scan_dir(&dir.join(&name), &child_rel, depth + 1, hasher)
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
struct FileRow {
    rel: String,
    is_dir: bool,
}

/// The `FileSystem` dock.
pub fn filesystem_dock() -> Part {
    let tabs = vec![tab("FileSystem", true)];
    let tail = vec![refresh_menu()];

    let body = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
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
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::scroll_y(),
            }
            ScrollArea
            Children [ {file_rows()} ]
        ]
    });

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            {tab_strip(tabs, tail, tokens::WINDOW)}
            --
            {body}
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

fn refresh_menu() -> Part {
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
    if tree.host == Some(host) && tree.built == tree.rev {
        return;
    }
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

        let scan = scan(&tmp.0);
        let rows = rows(&scan, &HashSet::new());
        assert_eq!(names(&rows), ["adir", "zdir", "a.png", "b.rs"]);
        assert!(rows.iter().all(|row| matches!(row, Row::Entry { .. })));
    }

    #[test]
    fn recursion_stops_at_the_depth_cap() {
        let tmp = Tmp::new("depth");
        tmp.dir("d1/d2/d3/d4/d5/d6");

        let scan = scan(&tmp.0);
        let expanded = open(&["d1", "d1/d2", "d1/d2/d3", "d1/d2/d3/d4", "d1/d2/d3/d4/d5"]);
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

        let scan = scan(&tmp.0);
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

        let scan = scan(&tmp.0);
        let rows = rows(&scan, &open(&["many"]));
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

        let first = scan(&tmp.0).signature;
        assert_eq!(scan(&tmp.0).signature, first, "an idle scan must not churn");

        tmp.file("b.rs", "fn b() {}");
        assert_ne!(scan(&tmp.0).signature, first);
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
        let scan = scan(&tmp.0.join("absent"));
        assert!(scan.missing);
        assert_eq!(rows(&scan, &HashSet::new()), [Row::MissingRoot]);
    }
}
