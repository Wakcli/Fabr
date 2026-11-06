//! UI-agnostic editor state: projects, layout, history, selection, and themes.

extern crate alloc;

/// Undo and redo stacks.
pub mod history;
/// Panel tree and split ratios.
pub mod layout;
/// Project hub, recents, and filtering.
pub mod project;
/// Multi-selection state.
pub mod selection;
/// Token palettes and color parsing.
pub mod theme;

pub use history::{History, Op, DEFAULT_LIMIT};
pub use layout::{Axis, Edge, Layout, PanelId, PanelKind, PanelTab, MAX_RATIO, MIN_RATIO};
pub use project::{HubState, Project, ProjectId};
pub use selection::Selection;
pub use theme::{Rgba, Theme, ThemeError};

/// Version of this crate.
pub const EDITOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS: [&str; 14] = [
        "surface.window",
        "surface.pane_header",
        "surface.pane_body",
        "text.default",
        "text.dim",
        "border.default",
        "fill.accent",
        "fill.solid",
        "fill.item_hover",
        "fill.field",
        "axis.x",
        "axis.y",
        "axis.z",
        "focus.ring",
    ];

    fn asset(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/editor")
            .join(name)
    }

    #[test]
    fn theme_asset_loads_with_every_token() {
        let theme = Theme::load_from_path(&asset("theme.ron")).unwrap();

        assert_eq!(theme.base, "dark");
        for token in TOKENS {
            assert!(theme.get(token).is_some(), "missing {token}");
        }
        assert!(theme.colors.len() >= TOKENS.len());
    }

    #[test]
    fn layout_asset_round_trips() {
        let src = std::fs::read_to_string(asset("layout.ron")).unwrap();
        let layout: Layout = ron::from_str(&src).unwrap();

        assert_eq!(layout, Layout::default_editor());
    }
}
