//! Editor chrome tokens, keyed to `assets/editor/theme.ron`.
//!
//! The chrome is neutral; hue is reserved for meaning. Class badges, transform
//! axes, folder glyphs, and the single warm accent are the only colored tokens.
//! Everything else is a neutral gray stepped by luminance.

use eme::feathers::theme::ThemeToken;

/// Menu bar, dock header strips, scene tabs strip, unselected dock tabs.
pub const WINDOW: ThemeToken = ThemeToken::new_static("editor.window");
/// Dock bodies, the active dock tab, and selected scene tabs.
pub const PANEL: ThemeToken = ThemeToken::new_static("editor.panel");
/// Tree, log, and file list bodies, one step darker than [`PANEL`].
pub const INSET: ThemeToken = ThemeToken::new_static("editor.inset");
/// One pixel separators between docks and around the window.
pub const BORDER: ThemeToken = ThemeToken::new_static("editor.border");
/// Hairline between controls inside a strip.
pub const SEPARATOR: ThemeToken = ThemeToken::new_static("editor.separator");

/// Primary editor text.
pub const TEXT: ThemeToken = ThemeToken::new_static("editor.text");
/// Selected tree row text, which sits on [`SELECT`].
pub const SELECT_TEXT: ThemeToken = ThemeToken::new_static("editor.select.text");
/// Secondary text, for labels that should recede without disappearing.
pub const TEXT_DIM: ThemeToken = ThemeToken::new_static("editor.text.dim");
/// Readable but recessed text, for inactive tabs and file names.
pub const TEXT_MEDIUM: ThemeToken = ThemeToken::new_static("editor.text.medium");
/// The single brand accent: active screen, active tab underline, focus rings.
pub const ACCENT: ThemeToken = ThemeToken::new_static("editor.accent");
/// Label text and glyphs drawn on top of [`ACCENT`].
pub const ACCENT_TEXT: ThemeToken = ThemeToken::new_static("editor.accent.text");
/// The two pixel underline under the active tab.
pub const TAB_HIGHLIGHT: ThemeToken = ThemeToken::new_static("editor.tab.highlight");
/// Tree row selection: the accent at low strength over [`INSET`].
pub const SELECT: ThemeToken = ThemeToken::new_static("editor.select");
/// Dims the editor behind a modal dialog.
pub const SCRIM: ThemeToken = ThemeToken::new_static("editor.scrim");
/// Fully transparent, for rows that only paint when selected.
pub const CLEAR: ThemeToken = ThemeToken::new_static("editor.clear");

/// Number field and line edit background.
pub const FIELD: ThemeToken = ThemeToken::new_static("editor.field");
/// Two pixel underline under fields.
pub const FIELD_LINE: ThemeToken = ThemeToken::new_static("editor.field.line");
/// Spin slider label cell.
pub const LABEL_STRIP: ThemeToken = ThemeToken::new_static("editor.label.strip");

/// Inspector category header.
pub const CATEGORY: ThemeToken = ThemeToken::new_static("editor.category");
/// Inspector section title text.
pub const SECTION_TEXT: ThemeToken = ThemeToken::new_static("editor.section.text");
/// Inspector property label text.
pub const PROP_TEXT: ThemeToken = ThemeToken::new_static("editor.prop.text");

/// X axis: red.
pub const AXIS_X: ThemeToken = ThemeToken::new_static("editor.axis.x");
/// Y axis: green.
pub const AXIS_Y: ThemeToken = ThemeToken::new_static("editor.axis.y");
/// Z axis: blue.
pub const AXIS_Z: ThemeToken = ThemeToken::new_static("editor.axis.z");
/// X axis component letter.
pub const AXIS_X_TEXT: ThemeToken = ThemeToken::new_static("editor.axis.x.text");
/// Y axis component letter.
pub const AXIS_Y_TEXT: ThemeToken = ThemeToken::new_static("editor.axis.y.text");
/// Z axis component letter.
pub const AXIS_Z_TEXT: ThemeToken = ThemeToken::new_static("editor.axis.z.text");

/// Top of the viewport gradient.
pub const VIEWPORT_TOP: ThemeToken = ThemeToken::new_static("editor.viewport.top");
/// Bottom of the viewport gradient.
pub const VIEWPORT_BOTTOM: ThemeToken = ThemeToken::new_static("editor.viewport.bottom");
/// Perspective grid line.
pub const GRID: ThemeToken = ThemeToken::new_static("editor.grid");
/// Horizon and axis grid lines.
pub const GRID_STRONG: ThemeToken = ThemeToken::new_static("editor.grid.strong");

/// Neutral icon tint.
pub const ICON: ThemeToken = ThemeToken::new_static("editor.icon");
/// Folder glyphs in the `FileSystem` dock.
pub const FOLDER: ThemeToken = ThemeToken::new_static("editor.icon.folder");

/// Class badge for scene-tree nodes.
pub const CLASS_NODE: ThemeToken = ThemeToken::new_static("editor.class.node");
/// Class badge for scenes.
pub const CLASS_SCENE: ThemeToken = ThemeToken::new_static("editor.class.scene");
/// Class badge for lights.
pub const CLASS_LIGHT: ThemeToken = ThemeToken::new_static("editor.class.light");
/// Class badge for cameras.
pub const CLASS_CAMERA: ThemeToken = ThemeToken::new_static("editor.class.camera");
/// Class badge for meshes.
pub const CLASS_MESH: ThemeToken = ThemeToken::new_static("editor.class.mesh");

/// Every editor token paired with the name it is stored under in the theme file.
pub const ALL: &[(&str, ThemeToken)] = &[
    ("editor.window", WINDOW),
    ("editor.panel", PANEL),
    ("editor.inset", INSET),
    ("editor.border", BORDER),
    ("editor.separator", SEPARATOR),
    ("editor.text", TEXT),
    ("editor.select.text", SELECT_TEXT),
    ("editor.text.dim", TEXT_DIM),
    ("editor.text.medium", TEXT_MEDIUM),
    ("editor.accent", ACCENT),
    ("editor.accent.text", ACCENT_TEXT),
    ("editor.tab.highlight", TAB_HIGHLIGHT),
    ("editor.select", SELECT),
    ("editor.scrim", SCRIM),
    ("editor.clear", CLEAR),
    ("editor.field", FIELD),
    ("editor.field.line", FIELD_LINE),
    ("editor.label.strip", LABEL_STRIP),
    ("editor.category", CATEGORY),
    ("editor.section.text", SECTION_TEXT),
    ("editor.prop.text", PROP_TEXT),
    ("editor.axis.x", AXIS_X),
    ("editor.axis.y", AXIS_Y),
    ("editor.axis.z", AXIS_Z),
    ("editor.axis.x.text", AXIS_X_TEXT),
    ("editor.axis.y.text", AXIS_Y_TEXT),
    ("editor.axis.z.text", AXIS_Z_TEXT),
    ("editor.viewport.top", VIEWPORT_TOP),
    ("editor.viewport.bottom", VIEWPORT_BOTTOM),
    ("editor.grid", GRID),
    ("editor.grid.strong", GRID_STRONG),
    ("editor.icon", ICON),
    ("editor.icon.folder", FOLDER),
    ("editor.class.node", CLASS_NODE),
    ("editor.class.scene", CLASS_SCENE),
    ("editor.class.light", CLASS_LIGHT),
    ("editor.class.camera", CLASS_CAMERA),
    ("editor.class.mesh", CLASS_MESH),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_their_tokens() {
        for (name, token) in ALL {
            assert_eq!(token.to_string(), *name);
        }
    }

    #[test]
    fn theme_asset_carries_every_token() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/editor/theme.ron");
        let theme = eme_editor_core::Theme::load_from_path(&path).unwrap();

        for (name, _) in ALL {
            assert!(theme.get(name).is_some(), "theme.ron is missing {name}");
        }
    }
}
