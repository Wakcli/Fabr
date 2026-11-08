//! Object context menu, shared by the Scene tree, the viewport, and the dock
//! toolbar's add button.

use eme::color::{Alpha, Color};
use eme::feathers::controls::{FeathersTextInput, FeathersTextInputContainer};
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::feathers::tokens as feather;
use eme::input_focus::{FocusCause, InputFocus};
use eme::picking::hover::Hovered;
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{EditableText, TextEdit};
use eme::ui::InteractionDisabled;
use eme::ui_widgets::{Activate, Button, SelectAllOnFocus};
use eme_game_scene::ObjectKind;

use super::chrome::*;
use super::icons;
use crate::clipboard::{self, Clipboard};
use crate::console::ConsoleLog;
use crate::persist;
use crate::scene::{self, SceneState};
use crate::tokens;
use crate::undo::Undo;

/// Menu row height, tight enough for a full object menu.
const ROW_H: f32 = 24.0;
/// Minimum panel width so short labels still read as a menu.
const PANEL_W: f32 = 206.0;
/// A right press that moves further than this was a camera drag, not a click.
const DRAG_SLOP: f32 = 4.0;

/// What a menu row does when it is activated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Save,
    Open,
    Copy,
    Cut,
    Paste,
    Rename,
    Duplicate,
    Delete,
    Create(ObjectKind),
    Page(Page),
    Close,
}

/// Which rows the popup shows. Submenus replace the contents in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Page {
    #[default]
    Root,
    Objects,
    Lights,
}

/// A row icon, built on spawn because scene parts are not cloneable.
type Icon = fn() -> Part;

/// One line in the menu: an actionable row or a separator.
enum Entry {
    Row {
        label: &'static str,
        act: Act,
        enabled: bool,
        icon: Option<Icon>,
        submenu: bool,
    },
    Divider,
}

impl Entry {
    fn row(label: &'static str, act: Act, enabled: bool, icon: Option<Icon>) -> Self {
        Self::Row {
            label,
            act,
            enabled,
            icon,
            submenu: false,
        }
    }

    fn submenu(label: &'static str, page: Page) -> Self {
        Self::Row {
            label,
            act: Act::Page(page),
            enabled: true,
            icon: None,
            submenu: true,
        }
    }
}

/// Where and for which object the menu is open.
#[derive(Clone, Copy)]
struct Open {
    pos: Vec2,
    target: Option<u64>,
    page: Page,
}

/// An in-flight rename popup.
#[derive(Clone)]
struct Rename {
    target: u64,
    seed: String,
}

/// Open menu and rename popup state.
#[derive(Resource, Default)]
pub struct MenuState {
    open: Option<Open>,
    rename: Option<Rename>,
}

/// Opens the menu at a window position, acting on `target` when given.
pub fn open(state: &mut MenuState, pos: Vec2, target: Option<u64>) {
    state.open = Some(Open {
        pos,
        target,
        page: Page::Root,
    });
}

/// Root of the popup tree; the sync system swaps it when the state changes.
#[derive(Component, Clone, Copy, Default)]
pub struct MenuRoot;

/// Marks a menu row and the action it carries.
#[derive(Component, Clone, Copy)]
pub struct MenuRow(Act);

impl Default for MenuRow {
    fn default() -> Self {
        Self(Act::Close)
    }
}

/// The text input of the rename popup, and what it renames.
#[derive(Component, Clone, Copy, Default)]
pub struct RenameInput(pub u64);

/// The backdrop that dismisses the menu or cancels a rename.
#[derive(Component, Clone, Copy, Default)]
pub enum Dismiss {
    #[default]
    Menu,
    Rename,
}

fn menu_icon(glyph: &'static str) -> Part {
    icons::mono(glyph, tokens::ICON)
}

fn mesh_icon() -> Part {
    icons::badge(icons::MESH)
}

fn light_icon() -> Part {
    icons::badge(icons::LIGHT)
}

fn camera_icon() -> Part {
    icons::badge(icons::CAMERA)
}

fn sphere_icon() -> Part {
    menu_icon(icons::SPHERE)
}

fn cylinder_icon() -> Part {
    menu_icon(icons::CYLINDER)
}

fn plane_icon() -> Part {
    menu_icon(icons::PLANE)
}

fn duplicate_icon() -> Part {
    menu_icon(icons::DUPLICATE)
}

fn rename_icon() -> Part {
    menu_icon(icons::RENAME)
}

fn empty_icon() -> Part {
    menu_icon(icons::EMPTY)
}

fn entries(has_clip: bool, open: Open) -> Vec<Entry> {
    let has_target = open.target.is_some();
    match open.page {
        Page::Root => vec![
            Entry::row("Save Scene", Act::Save, true, None),
            Entry::row("Open Scene", Act::Open, true, None),
            Entry::Divider,
            Entry::row("Copy", Act::Copy, has_target, None),
            Entry::row("Cut", Act::Cut, has_target, None),
            Entry::row("Paste", Act::Paste, has_clip, None),
            Entry::row("Rename", Act::Rename, has_target, Some(rename_icon)),
            Entry::row(
                "Duplicate",
                Act::Duplicate,
                has_target,
                Some(duplicate_icon),
            ),
            Entry::row("Delete", Act::Delete, has_target, None),
            Entry::Divider,
            Entry::row(
                "Create Empty",
                Act::Create(ObjectKind::Empty),
                true,
                Some(empty_icon),
            ),
            Entry::Divider,
            Entry::submenu("3D Object", Page::Objects),
            Entry::submenu("Light", Page::Lights),
            Entry::row(
                "Camera",
                Act::Create(ObjectKind::Camera),
                true,
                Some(camera_icon),
            ),
            Entry::Divider,
            Entry::row("Effects", Act::Close, false, None),
            Entry::row("Audio", Act::Close, false, None),
            Entry::row("Video", Act::Close, false, None),
            Entry::row("UI", Act::Close, false, None),
            Entry::row("AI", Act::Close, false, None),
        ],
        Page::Objects => {
            let mut rows = page_header("Back");
            for kind in ObjectKind::ALL {
                let icon: Icon = match kind {
                    ObjectKind::Cube => mesh_icon,
                    ObjectKind::Sphere => sphere_icon,
                    ObjectKind::Cylinder => cylinder_icon,
                    ObjectKind::Plane => plane_icon,
                    _ => continue,
                };
                rows.push(Entry::row(
                    kind.label(),
                    Act::Create(kind),
                    true,
                    Some(icon),
                ));
            }
            rows.push(Entry::row("Polygon", Act::Close, false, None));
            rows
        }
        Page::Lights => {
            let mut rows = page_header("Back");
            for kind in ObjectKind::ALL {
                if !matches!(kind, ObjectKind::PointLight | ObjectKind::Sun) {
                    continue;
                }
                rows.push(Entry::row(
                    kind.label(),
                    Act::Create(kind),
                    true,
                    Some(light_icon),
                ));
            }
            rows
        }
    }
}

fn page_header(label: &'static str) -> Vec<Entry> {
    vec![
        Entry::row(label, Act::Page(Page::Root), true, None),
        Entry::Divider,
    ]
}

/// Rebuilds the popup whenever the menu state changes.
pub fn sync_menu(
    mut commands: Commands,
    state: Res<MenuState>,
    clip: Res<Clipboard>,
    roots: Query<Entity, With<MenuRoot>>,
) {
    if !state.is_changed() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    if let Some(open) = state.open {
        spawn_menu(&mut commands, &clip, open);
    }
    if let Some(rename) = state.rename.clone() {
        spawn_rename(&mut commands, &rename);
    }
}

fn spawn_menu(commands: &mut Commands, clip: &Clipboard, open: Open) {
    let root = commands.spawn_scene(root_layer()).id();
    commands.entity(root).insert(MenuRoot);
    let backdrop = commands
        .spawn_scene(backdrop(Dismiss::Menu, tokens::CLEAR))
        .id();
    commands.entity(backdrop).insert(ChildOf(root));
    let panel = commands.spawn_scene(menu_panel(open.pos)).id();
    commands.entity(panel).insert(ChildOf(root));

    for entry in entries(!clip.0.is_empty(), open) {
        match entry {
            Entry::Divider => {
                commands.spawn_scene(divider(panel));
            }
            Entry::Row {
                label,
                act,
                enabled,
                icon,
                submenu,
            } => {
                let row = commands
                    .spawn_scene(menu_row(panel, label, act, enabled, icon, submenu))
                    .id();
                if !enabled {
                    commands.entity(row).insert(InteractionDisabled);
                }
            }
        }
    }
}

fn spawn_rename(commands: &mut Commands, rename: &Rename) {
    let root = commands.spawn_scene(root_layer()).id();
    commands.entity(root).insert(MenuRoot);
    let backdrop = commands
        .spawn_scene(backdrop(Dismiss::Rename, tokens::CLEAR))
        .id();
    commands.entity(backdrop).insert(ChildOf(root));
    let card = commands.spawn_scene(rename_card(rename.target)).id();
    commands.entity(card).insert(ChildOf(root));
}

/// A transparent full-window layer that keeps the popup out of dock clipping.
fn root_layer() -> impl Scene {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
        }
        GlobalZIndex(100)
        Pickable::IGNORE
    })
}

fn backdrop(dismiss: Dismiss, token: ThemeToken) -> impl Scene {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
        }
        ThemeBackgroundColor(token)
        Button
        dismiss
        on(dismiss_popup)
    })
}

fn menu_panel(pos: Vec2) -> impl Scene {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(pos.x),
            top: px(pos.y),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            min_width: px(PANEL_W),
            padding: UiRect::axes(px(4), px(4)),
            border: px(1),
            border_radius: px(4),
        }
        ThemeBackgroundColor(feather::MENU_BG)
        ThemeBorderColor(feather::MENU_BORDER)
        GlobalZIndex(100)
        OverrideClip
        BoxShadow::new(Color::BLACK.with_alpha(0.55), px(0), px(0), px(2), px(8))
    })
}

fn menu_row(
    parent: Entity,
    label: &'static str,
    act: Act,
    enabled: bool,
    icon: Option<Icon>,
    submenu: bool,
) -> impl Scene {
    // the feathers menuitem text token resolves to the on-accent color, which the
    // editor theme aliases to the dark window color; the editor's own text tokens
    // are the ones that stay readable on a menu surface
    let fg = if enabled {
        tokens::TEXT
    } else {
        tokens::TEXT_DIM
    };
    let mut kids = Vec::new();
    match icon {
        Some(build) => kids.push(build()),
        None => kids.push(rect(16.0, 16.0, tokens::CLEAR, 0.0)),
    }
    kids.push(text(label, FONT_SIZE, fg));
    kids.push(spacer());
    if submenu {
        kids.push(icons::mono(icons::CHEVRON_RIGHT, tokens::ICON));
    }

    Box::new(bsn! {
        Node {
            height: px(ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            column_gap: px(8),
            border_radius: px(2),
        }
        ThemeBackgroundColor(feather::MENUITEM_BG)
        Button
        Hovered
        MenuRow(act)
        ChildOf(parent)
        Children [ {kids} ]
        on(activate_row)
    })
}

fn divider(parent: Entity) -> impl Scene {
    bsn! {
        Node {
            height: px(1),
            margin: UiRect::vertical(px(3)),
        }
        ThemeBackgroundColor(feather::MENU_BORDER)
        ChildOf(parent)
    }
}

fn rename_card(target: u64) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            margin: UiRect {
                left: px(-180),
                top: px(-56),
            },
            width: px(360),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(8),
            padding: px(12),
            border: px(1),
            border_radius: px(6),
        }
        ThemeBackgroundColor(feather::MENU_BG)
        ThemeBorderColor(feather::MENU_BORDER)
        GlobalZIndex(100)
        OverrideClip
        BoxShadow::new(Color::BLACK.with_alpha(0.55), px(0), px(0), px(2), px(10))
        Children [
            {text("Rename object", FONT_SIZE, tokens::TEXT)}
            --
            Node {
                height: px(30),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                border: px(1),
                border_radius: px(4),
            }
            ThemeBackgroundColor(tokens::FIELD)
            ThemeBorderColor(tokens::FIELD_LINE)
            Children [
                @FeathersTextInputContainer
                Children [
                    @FeathersTextInput
                    SelectAllOnFocus
                    RenameInput(target)
                ]
            ]
            --
            {text(
                "Enter to confirm, Esc to cancel",
                FONT_SIZE_OUTPUT,
                tokens::TEXT_DIM,
            )}
        ]
    }
}

/// Highlights the row under the pointer.
pub fn style_menu_rows(
    rows: Query<
        (
            Entity,
            Has<InteractionDisabled>,
            &Hovered,
            &ThemeBackgroundColor,
        ),
        With<MenuRow>,
    >,
    mut commands: Commands,
) {
    for (row, disabled, hovered, current) in &rows {
        let want = if disabled || !hovered.0 {
            feather::MENUITEM_BG
        } else {
            feather::MENUITEM_BG_HOVER
        };
        if current.0 != want {
            commands.entity(row).insert(ThemeBackgroundColor(want));
        }
    }
}

fn dismiss_popup(dismiss: On<Activate>, kinds: Query<&Dismiss>, mut state: ResMut<MenuState>) {
    let Ok(kind) = kinds.get(dismiss.entity) else {
        return;
    };
    match kind {
        Dismiss::Menu => state.open = None,
        Dismiss::Rename => state.rename = None,
    }
}

fn activate_row(
    activate: On<Activate>,
    rows: Query<&MenuRow>,
    mut menu: ResMut<MenuState>,
    mut scene: ResMut<SceneState>,
    mut clip: ResMut<Clipboard>,
    mut undo: ResMut<Undo>,
    index: Res<scene::ModelIndex>,
    mut log: ResMut<ConsoleLog>,
) {
    let Ok(MenuRow(act)) = rows.get(activate.entity) else {
        return;
    };
    let Some(open) = menu.open else {
        return;
    };
    let target = open.target;

    match *act {
        Act::Save => {
            match persist::save_to(&scene.doc, persist::SCENE_PATH) {
                Ok(path) => info!("saved scene to {}", path.display()),
                Err(err) => error!("could not save scene: {err}"),
            }
            menu.open = None;
        }
        Act::Open => {
            persist::open_path_into(&mut scene, persist::SCENE_PATH, &index, &mut log);
            menu.open = None;
        }
        Act::Copy => {
            clipboard::copy_selection(&scene, &mut clip);
            menu.open = None;
        }
        Act::Cut => {
            clipboard::cut_selection(&mut scene, &mut clip, &mut undo);
            menu.open = None;
        }
        Act::Paste => {
            clipboard::paste(&mut scene, &clip, &mut undo);
            menu.open = None;
        }
        Act::Duplicate => {
            clipboard::duplicate_selection(&mut scene, &mut undo);
            menu.open = None;
        }
        Act::Delete => {
            if target.is_none() {
                return;
            }
            undo.checkpoint(&scene, "Delete");
            scene.despawn_selected();
            menu.open = None;
        }
        Act::Rename => {
            let Some(id) = target else {
                return;
            };
            let Some(obj) = scene.object(id) else {
                return;
            };
            menu.rename = Some(Rename {
                target: id,
                seed: obj.name.clone(),
            });
            menu.open = None;
        }
        Act::Create(kind) => {
            undo.checkpoint(&scene, "Create");
            let id = scene.spawn(kind);
            scene.select_only(id);
            menu.open = None;
        }
        Act::Page(page) => {
            menu.open = Some(Open { page, ..open });
        }
        Act::Close => {}
    }
}

/// Seeds the popup input with the current name and gives it the keyboard.
pub fn seed_rename(
    menu: Res<MenuState>,
    mut inputs: Query<(Entity, &mut EditableText, &RenameInput), Added<RenameInput>>,
    mut focus: ResMut<InputFocus>,
) {
    let Some(rename) = menu.rename.as_ref() else {
        return;
    };
    for (entity, mut input, seat) in &mut inputs {
        if seat.0 != rename.target {
            continue;
        }
        input.queue_edit(TextEdit::Insert(rename.seed.as_str().into()));
        focus.set(entity, FocusCause::Navigated);
    }
}

/// Commits the rename on Enter and throws it away on Escape.
pub fn rename_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<MenuState>,
    inputs: Query<(&RenameInput, &EditableText)>,
    mut scene: ResMut<SceneState>,
    mut focus: ResMut<InputFocus>,
) {
    if menu.rename.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        menu.rename = None;
        focus.clear();
        return;
    }
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    let Ok((seat, input)) = inputs.single() else {
        return;
    };
    let name = input.value().to_string().trim().to_owned();
    let target = seat.0;
    menu.rename = None;
    focus.clear();
    if name.is_empty() {
        return;
    }
    if !scene.doc.rename(target, name) {
        return;
    }
    scene.touch();
}

/// The dock toolbar's add button, which opens the create menu at the pointer.
pub fn add_button() -> Part {
    Box::new(bsn! {
        Node {
            width: px(24),
            height: px(24),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Button
        Children [ {icons::add()} ]
        on(open_create_menu)
    })
}

fn open_create_menu(press: On<PointerPress>, mut menu: ResMut<MenuState>, scene: Res<SceneState>) {
    if press.button != PointerButton::Primary {
        return;
    }
    open(&mut menu, press.pointer.position, scene.primary());
}

/// Right press origin, so a camera orbit is not mistaken for a menu click.
#[derive(Resource, Default)]
pub struct RightPress(Option<Vec2>);

/// Records where a secondary press started on the viewport.
pub fn arm_viewport_menu(press: On<PointerPress>, mut armed: ResMut<RightPress>) {
    if press.button != PointerButton::Secondary {
        return;
    }
    armed.0 = Some(press.pointer.position);
}

/// Opens the viewport menu when the secondary press did not become an orbit.
pub fn viewport_menu(
    click: On<PointerClick>,
    mut armed: ResMut<RightPress>,
    mut menu: ResMut<MenuState>,
    scene: Res<SceneState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    if click.duration.as_millis() > 250 {
        armed.0 = None;
        return;
    }
    let Some(start) = armed.0.take() else {
        return;
    };
    if click.pointer.position.distance(start) > DRAG_SLOP {
        return;
    }
    open(&mut menu, start, scene.primary());
}

/// A right click on an object asks for the menu for that object.
pub fn request_object_menu(request: On<scene::ObjectMenu>, mut menu: ResMut<MenuState>) {
    open(&mut menu, request.pos, Some(request.id));
}

/// Drops the popup when the editor screen goes away.
pub fn close_on_teardown(
    panels: Query<(), With<super::viewport::ViewportPanel>>,
    mut menu: ResMut<MenuState>,
) {
    if panels.is_empty() {
        menu.open = None;
        menu.rename = None;
    }
}

/// Top-level menu the user can open.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TopKind {
    /// Scene commands.
    #[default]
    Scene,
    /// Debug commands.
    Debug,
    /// Editor commands.
    Editor,
    /// Help commands.
    Help,
}

/// Open top-level menu state.
#[derive(Resource, Clone, Default)]
pub struct TopMenu {
    /// Which menu is open, if any.
    pub open: Option<TopKind>,
    /// Window position.
    pub at: Vec2,
}

/// Button in the menu bar that opens a top-level menu.
#[derive(Component, Clone, Copy, Default)]
pub struct TopMenuButton(pub TopKind);

/// Root of the top-level menu overlay.
#[derive(Component, Clone, Copy, Default)]
pub struct TopMenuRoot;

/// Host for rebuilt menu rows.
#[derive(Component, Clone, Copy, Default)]
pub struct TopMenuRows;

/// One command in a top-level menu.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopAction {
    /// Save the current scene.
    SaveScene,
    /// Open `scenes/main.ember`.
    OpenScene,
    /// Add one cube.
    AddCube,
    /// Add an empty object.
    AddEmpty,
    /// Clear the console.
    ClearConsole,
    /// Reload all Lua scripts.
    ReloadScripts,
    /// Open project settings.
    ProjectSettings,
    /// Open the build dialog.
    Build,
    /// Open the audio mixer.
    AudioMixer,
    /// Today's build, in the console.
    About,
}

/// Opens a top-level menu under its button.
pub fn open_top_menu(
    activate: On<Activate>,
    buttons: Query<&TopMenuButton>,
    mut menu: ResMut<TopMenu>,
    mut import: ResMut<crate::screens::editor::files::ImportMenu>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let x = match button.0 {
        TopKind::Scene => 72.0,
        TopKind::Debug => 190.0,
        TopKind::Editor => 260.0,
        TopKind::Help => 330.0,
    };
    menu.open = Some(button.0);
    menu.at = Vec2::new(x, 34.0);
    import.at = None;
}

/// Shows or hides the overlay and rebuilds its rows.
pub fn sync_top_menu(
    menu: Res<TopMenu>,
    mut roots: Query<&mut Node, With<TopMenuRoot>>,
    hosts: Query<Entity, With<TopMenuRows>>,
    mut commands: Commands,
    mut shown: Local<Option<(Entity, Option<TopKind>, Vec2)>>,
) {
    for mut node in &mut roots {
        let display = if menu.open.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        node.left = px(menu.at.x);
        node.top = px(menu.at.y);
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let signature = (host, menu.open, menu.at);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    despawn_children(&mut commands, host);
    let Some(kind) = menu.open else {
        return;
    };
    let entries: &[(&str, TopAction)] = match kind {
        TopKind::Scene => &[
            ("Save Scene", TopAction::SaveScene),
            ("Open Scene", TopAction::OpenScene),
            ("Add Cube", TopAction::AddCube),
            ("Add Empty", TopAction::AddEmpty),
        ],
        TopKind::Debug => &[
            ("Clear Console", TopAction::ClearConsole),
            ("Reload Scripts", TopAction::ReloadScripts),
        ],
        TopKind::Editor => &[
            ("Project Settings…", TopAction::ProjectSettings),
            ("Audio Mixer…", TopAction::AudioMixer),
        ],
        TopKind::Help => &[("About Fabr", TopAction::About)],
    };
    for (label, action) in entries {
        let label = text(*label, FONT_SIZE, tokens::TEXT);
        commands
            .spawn_scene(bsn! {
                Node {
                    height: px(24),
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(10)),
                    min_width: px(180),
                }
                Button
                ThemeBackgroundColor(tokens::PANEL)
                ChildOf(host)
                Children [ {label} ]
            })
            .insert(*action)
            .observe(run_top_action);
    }
}

/// Runs one top-level menu command and closes the menu.
pub fn run_top_action(
    activate: On<Activate>,
    actions: Query<&TopAction>,
    mut menu: ResMut<TopMenu>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut log: ResMut<ConsoleLog>,
    mut settings: ResMut<crate::settings::ProjectSettingsState>,
    mut build: ResMut<crate::build::BuildState>,
    mut mixer: ResMut<crate::mixer::MixerState>,
    mut reload: ResMut<eme_script::ScriptReload>,
    index: Res<scene::ModelIndex>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    menu.open = None;
    match action {
        TopAction::SaveScene => {
            match persist::save_to(&state.doc, persist::SCENE_PATH) {
                Ok(path) => log.push(format!("saved scene to {}", path.display())),
                Err(err) => log.push(format!("could not save scene: {err}")),
            }
        }
        TopAction::OpenScene => {
            persist::open_path_into(&mut state, persist::SCENE_PATH, &index, &mut log);
            undo.clear();
        }
        TopAction::AddCube => {
            undo.checkpoint(&state, "Add Cube");
            let id = state.spawn(ObjectKind::Cube);
            state.select_only(id);
            state.touch();
        }
        TopAction::AddEmpty => {
            undo.checkpoint(&state, "Add Empty");
            let id = state.spawn(ObjectKind::Empty);
            state.select_only(id);
            state.touch();
        }
        TopAction::ClearConsole => log.clear(),
        TopAction::ReloadScripts => {
            for path in eme_script::script_files(&crate::asset_root()) {
                reload.request(path);
            }
            log.push("reload requested for all scripts".to_string());
        }
        TopAction::ProjectSettings => crate::settings::open(&mut settings),
        TopAction::Build => build.open(),
        TopAction::AudioMixer => crate::mixer::open(&mut mixer),
        TopAction::About => log.push(format!(
            "Fabr {} — built on the Eme engine",
            env!("CARGO_PKG_VERSION")
        )),
    }
}

/// Closes the top-level menu on Escape.
pub fn close_top_menu(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<TopMenu>) {
    if keys.just_pressed(KeyCode::Escape) {
        menu.open = None;
    }
}

/// The top-level menu overlay.
pub fn top_menu() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(0),
            top: px(0),
            min_width: px(180),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(4),
            padding: px(2),
        }
        TopMenuRoot
        GlobalZIndex(310)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
            }
            TopMenuRows
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(rows: &[Entry], label: &str) -> Option<bool> {
        rows.iter().find_map(|entry| match entry {
            Entry::Row {
                label: row,
                enabled,
                ..
            } if *row == label => Some(*enabled),
            _ => None,
        })
    }

    #[test]
    fn clipboard_and_target_rows_follow_the_selection() {
        let empty = Open {
            pos: Vec2::ZERO,
            target: None,
            page: Page::Root,
        };
        let rows = entries(false, empty);
        assert_eq!(enabled(&rows, "Paste"), Some(false));
        assert_eq!(enabled(&rows, "Copy"), Some(false));
        assert_eq!(enabled(&rows, "Cut"), Some(false));
        assert_eq!(enabled(&rows, "Delete"), Some(false));

        let with_target = Open {
            target: Some(4),
            ..empty
        };
        let rows = entries(true, with_target);
        assert_eq!(enabled(&rows, "Paste"), Some(true));
        assert_eq!(enabled(&rows, "Copy"), Some(true));
        assert_eq!(enabled(&rows, "Cut"), Some(true));
        assert_eq!(enabled(&rows, "Delete"), Some(true));
        assert_eq!(enabled(&rows, "Rename"), Some(true));
    }
}
