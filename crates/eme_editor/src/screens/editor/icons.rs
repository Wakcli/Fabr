//! Original monochrome editor glyphs, authored as 1.5 px stroke vectors on a
//! 16 px grid and rasterized to PNG beside their sources in
//! `assets/editor/icons`.
//!
//! Every glyph is white on transparent and tinted from a theme token at use
//! site, so one asset serves neutral chrome, colored class badges, and selected
//! states without a second PNG.

use eme::color::Color;
use eme::feathers::display::ThemedIcon;
use eme::feathers::theme::{ThemeTextColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::TextColor;

use super::chrome::Part;
use crate::tokens;

macro_rules! png {
    ($($name:ident => $stem:literal),* $(,)?) => {
        $(pub const $name: &str = concat!("editor/icons/", $stem, ".png");)*
    };
}

png! {
    ACCOUNT => "account",
    ADD => "plus",
    BRANCH => "branch",
    CHECK => "check",
    CHEVRON_DOWN => "chevron_down",
    CHEVRON_RIGHT => "chevron_right",
    CLASS_CAMERA => "class_camera",
    CLASS_LIGHT => "class_light",
    CLASS_MESH => "class_mesh",
    CLASS_NODE => "class_node",
    CLASS_SCENE => "class_scene",
    CLOSE => "close",
    CYLINDER => "cylinder",
    DUPLICATE => "duplicate",
    EMPTY => "empty",
    FILE => "file",
    FOLDER => "folder",
    FOLDER_LARGE => "folder_large",
    GEAR => "gear",
    INSTANCE => "instance",
    LINK => "link",
    LOCK => "lock",
    MENU_DOTS => "menu_dots",
    NETWORK => "network",
    PLANE => "plane",
    PLAY => "play",
    PLAY_SCENE => "play_scene",
    RENAME => "rename",
    SEARCH => "search",
    SNAP => "snap",
    SPHERE => "sphere",
    TOOL_MOVE => "tool_move",
    TOOL_ROTATE => "tool_rotate",
    TOOL_SCALE => "tool_scale",
    TOOL_SELECT => "tool_select",
    TOOL_TRANSFORM => "tool_transform",
}

pub const LOGO: &str = "editor/logo.png";

fn image(path: &'static str, size: f32, tint: Option<ThemeToken>) -> Part {
    match tint {
        Some(token) => Box::new(bsn! {
            Node { width: px(size), height: px(size) }
            ImageNode { image: path }
            TextColor(Color::WHITE)
            ThemeTextColor(token)
            ThemedIcon
        }),
        None => Box::new(bsn! {
            Node { width: px(size), height: px(size) }
            ImageNode { image: path }
        }),
    }
}

/// A mono glyph modulated by a theme token.
pub fn mono(path: &'static str, token: ThemeToken) -> Part {
    image(path, 16.0, Some(token))
}

/// A mono glyph drawn at 12 px, such as the tree carets.
pub fn small(path: &'static str, token: ThemeToken) -> Part {
    image(path, 12.0, Some(token))
}

/// A class badge: the glyph and the token that colors it.
pub struct Badge {
    glyph: &'static str,
    tint: ThemeToken,
}

impl Badge {
    const fn new(glyph: &'static str, tint: ThemeToken) -> Self {
        Self { glyph, tint }
    }
}

pub const NODE: Badge = Badge::new(CLASS_NODE, tokens::CLASS_NODE);
pub const SCENE: Badge = Badge::new(CLASS_SCENE, tokens::CLASS_SCENE);
pub const LIGHT: Badge = Badge::new(CLASS_LIGHT, tokens::CLASS_LIGHT);
pub const CAMERA: Badge = Badge::new(CLASS_CAMERA, tokens::CLASS_CAMERA);
pub const MESH: Badge = Badge::new(CLASS_MESH, tokens::CLASS_MESH);

/// A class badge in the tree, the inspector header, or a scene tab.
pub fn badge(badge: Badge) -> Part {
    mono(badge.glyph, badge.tint)
}

pub fn logo() -> Part {
    Box::new(bsn! {
        Node { width: px(23), height: px(22) }
        ImageNode { image: LOGO }
    })
}

pub fn play() -> Part {
    mono(PLAY, tokens::ICON)
}

pub fn play_scene() -> Part {
    mono(PLAY_SCENE, tokens::ICON)
}

pub fn dropdown() -> Part {
    mono(CHEVRON_DOWN, tokens::ICON)
}

pub fn network() -> Part {
    mono(NETWORK, tokens::ICON)
}

pub fn branch() -> Part {
    mono(BRANCH, tokens::ICON)
}

pub fn gear() -> Part {
    mono(GEAR, tokens::ICON)
}

pub fn account() -> Part {
    mono(ACCOUNT, tokens::ICON)
}

pub fn add() -> Part {
    mono(ADD, tokens::ICON)
}

pub fn instance() -> Part {
    mono(INSTANCE, tokens::ICON)
}

pub fn attach_script() -> Part {
    mono(LINK, tokens::ICON)
}

pub fn search() -> Part {
    mono(SEARCH, tokens::ICON)
}

/// The magnifier inside the filter field, which reads as placeholder text.
pub fn search_dim() -> Part {
    mono(SEARCH, tokens::TEXT_DIM)
}

pub fn close() -> Part {
    mono(CLOSE, tokens::ICON)
}

pub fn dock_menu() -> Part {
    mono(MENU_DOTS, tokens::ICON)
}

pub fn lock() -> Part {
    mono(LOCK, tokens::ICON)
}

pub fn check() -> Part {
    mono(CHECK, tokens::ICON)
}

/// Tree caret: down when expanded, right when collapsed.
pub fn caret(open: bool) -> Part {
    caret_tinted(open, tokens::ICON)
}

/// Tree caret in an explicit tint, for rows where the icon must follow the text.
pub fn caret_tinted(open: bool, token: ThemeToken) -> Part {
    small(if open { CHEVRON_DOWN } else { CHEVRON_RIGHT }, token)
}

/// Right-pointing arrow used between breadcrumb segments.
pub fn breadcrumb_caret() -> Part {
    small(CHEVRON_RIGHT, tokens::TEXT_DIM)
}

pub fn folder() -> Part {
    mono(FOLDER, tokens::FOLDER)
}

pub fn folder_large() -> Part {
    image(FOLDER_LARGE, 40.0, Some(tokens::FOLDER))
}

pub fn file() -> Part {
    mono(FILE, tokens::TEXT_MEDIUM)
}
