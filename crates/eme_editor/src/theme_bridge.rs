use eme::color::{Alpha, Color, Luminance};
use eme::feathers::{
    dark_theme::create_dark_theme,
    theme::{SemanticToken, ThemeProps},
    tokens::{self, semantic},
};
use eme::prelude::*;
use eme_editor_core::{Rgba, Theme};

const HOVER_LIFT: f32 = 0.05;
const PRESS_LIFT: f32 = 0.1;
const DISABLED_ALPHA: f32 = 0.5;

fn semantic_for(name: &str) -> Option<SemanticToken> {
    Some(match name {
        "surface.window" => semantic::SURFACE_WINDOW,
        "surface.pane_header" => semantic::SURFACE_PANE_HEADER,
        "surface.pane_body" => semantic::SURFACE_PANE_BODY,
        "text.default" => semantic::TEXT_DEFAULT,
        "text.dim" => semantic::TEXT_DIM,
        "border.default" => semantic::BORDER_DEFAULT,
        "fill.accent" => semantic::FILL_ACCENT_DEFAULT,
        "fill.solid" => semantic::FILL_SOLID_DEFAULT,
        "fill.item_hover" => semantic::FILL_ITEM_HOVER,
        "fill.field" => semantic::FILL_FIELD_DEFAULT,
        "axis.x" => semantic::AXIS_X,
        "axis.y" => semantic::AXIS_Y,
        "axis.z" => semantic::AXIS_Z,
        "focus.ring" => semantic::FOCUS_RING,
        _ => return None,
    })
}

fn color(rgba: Rgba) -> Color {
    Color::srgb_u8(rgba.r, rgba.g, rgba.b).with_alpha(rgba.a as f32 / 255.0)
}

/// Converts an editor core theme into feathers theme properties.
pub fn props(theme: &Theme) -> ThemeProps {
    let mut props = create_dark_theme();
    props.semantic_base.clear();
    props.semantic_overrides.clear();

    let uses_editor_palette = theme.colors.keys().any(|name| name.starts_with("editor."));

    for (name, rgba) in &theme.colors {
        if let Some(token) = semantic_for(name) {
            props.semantic_base.insert(token, color(*rgba));
        } else if !uses_editor_palette {
            warn!("editor theme token `{name}` has no feathers mapping");
        }
    }

    if uses_editor_palette {
        for (name, tok) in crate::tokens::ALL {
            let Some(rgba) = theme.get(name) else {
                warn!("editor theme is missing `{name}`");
                continue;
            };
            let semantic = SemanticToken::new_static(name);
            props
                .token_assignments
                .insert(tok.clone(), semantic.clone());
            props.semantic_base.insert(semantic, color(rgba));
        }
    }

    props
        .semantic_base
        .insert(semantic::FILL_ITEM_DEFAULT, Color::NONE);
    props.semantic_base.insert(semantic::FILL_NONE, Color::NONE);

    // The dark theme paints button and list text with the on-accent color. In a
    // monochrome shell the accent is white and secondary surfaces stay dark, so
    // those widgets need the default text color instead.
    props
        .token_assignments
        .insert(tokens::BUTTON_TEXT, semantic::TEXT_DEFAULT);
    props
        .token_assignments
        .insert(tokens::LISTROW_TEXT, semantic::TEXT_DEFAULT);
    // A disabled primary keeps its dark label; only the accent fill fades, so
    // the call to action stays readable instead of gray-on-orange.
    props.token_assignments.insert(
        tokens::BUTTON_PRIMARY_TEXT_DISABLED,
        semantic::TEXT_ON_ACCENT,
    );

    let get = |name: &str| theme.get(name).map(color);
    let mut alias = |token: SemanticToken, from: &str| {
        if let Some(value) = get(from) {
            props.semantic_base.insert(token, value);
        }
    };

    alias(semantic::SURFACE_SUBPANE_HEADER, "surface.pane_header");
    alias(semantic::SURFACE_SUBPANE_BODY, "surface.pane_body");
    alias(semantic::SURFACE_GROUP, "surface.pane_header");
    alias(semantic::SURFACE_MENU, "surface.pane_header");
    alias(semantic::SURFACE_DIALOG, "surface.pane_body");
    alias(semantic::TEXT_ON_ACCENT, "surface.window");
    alias(semantic::TEXT_DISABLED, "text.dim");

    alias(semantic::FILL_ACCENT_HOVER, "fill.accent");
    alias(semantic::FILL_ACCENT_PRESSED, "fill.accent");
    alias(semantic::FILL_ACCENT_DISABLED, "fill.accent");
    alias(semantic::FILL_SOLID_HOVER, "fill.solid");
    alias(semantic::FILL_SOLID_PRESSED, "fill.solid");
    alias(semantic::FILL_SOLID_DISABLED, "fill.solid");
    alias(semantic::FILL_ITEM_PRESSED, "fill.item_hover");
    alias(semantic::FILL_FIELD_HOVER, "fill.field");
    alias(semantic::FILL_FIELD_PRESSED, "fill.field");

    if let Some(accent) = get("fill.accent") {
        props
            .semantic_base
            .insert(semantic::FILL_ACCENT_HOVER, accent.lighter(HOVER_LIFT));
        props
            .semantic_base
            .insert(semantic::FILL_ACCENT_PRESSED, accent.lighter(PRESS_LIFT));
        props.semantic_base.insert(
            semantic::FILL_ACCENT_DISABLED,
            accent.with_alpha(DISABLED_ALPHA),
        );
    }
    if let Some(solid) = get("fill.solid") {
        props
            .semantic_base
            .insert(semantic::FILL_SOLID_HOVER, solid.lighter(HOVER_LIFT));
        props
            .semantic_base
            .insert(semantic::FILL_SOLID_PRESSED, solid.lighter(PRESS_LIFT));
        props.semantic_base.insert(
            semantic::FILL_SOLID_DISABLED,
            solid.with_alpha(DISABLED_ALPHA),
        );
    }
    if let Some(item) = get("fill.item_hover") {
        props
            .semantic_base
            .insert(semantic::FILL_ITEM_PRESSED, item.lighter(PRESS_LIFT));
    }
    if let Some(field) = get("fill.field") {
        props
            .semantic_base
            .insert(semantic::FILL_FIELD_HOVER, field.lighter(HOVER_LIFT));
        props
            .semantic_base
            .insert(semantic::FILL_FIELD_PRESSED, field.lighter(PRESS_LIFT));
    }

    props
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme::feathers::tokens;

    fn theme() -> Theme {
        Theme::load_from_str(
            r##"(
                base: "dark",
                colors: {
                    "surface.window": "#121212",
                    "surface.pane_header": "#1B1B1B",
                    "surface.pane_body": "#141414",
                    "text.default": "#E6E6E6",
                    "text.dim": "#9A9A9A",
                    "border.default": "#2A2A2A",
                    "fill.accent": "#E8743B",
                    "fill.solid": "#3A3A3A",
                    "fill.item_hover": "#242424",
                    "fill.field": "#1F1F1F",
                    "focus.ring": "#E8743BCC",
                },
            )"##,
        )
        .unwrap()
    }

    #[test]
    fn maps_tokens_and_keeps_feathers_assignments() {
        let props = props(&theme());

        assert_eq!(
            props.semantic_base.get(&semantic::SURFACE_PANE_BODY),
            Some(&Color::srgb_u8(0x14, 0x14, 0x14))
        );
        assert_eq!(
            props.semantic_base.get(&semantic::TEXT_ON_ACCENT),
            Some(&Color::srgb_u8(0x12, 0x12, 0x12))
        );
        assert!(props
            .semantic_base
            .contains_key(&semantic::FILL_ACCENT_HOVER));
        assert!(props.token_assignments.contains_key(&tokens::PANE_BODY_BG));
    }

    #[test]
    fn maps_editor_chrome_tokens_through_their_own_names() {
        let theme = Theme::load_from_str(
            r##"(
                base: "dark",
                colors: {
                    "editor.panel": "#171717",
                    "editor.text": "#E8E8E8",
                },
            )"##,
        )
        .unwrap();

        let props = props(&theme);
        let panel = crate::tokens::PANEL;

        assert_eq!(
            props.token_assignments.get(&panel),
            Some(&SemanticToken::new_static("editor.panel"))
        );
        assert_eq!(
            props
                .semantic_base
                .get(&SemanticToken::new_static("editor.panel")),
            Some(&Color::srgb_u8(0x17, 0x17, 0x17))
        );
        assert_eq!(
            props
                .semantic_base
                .get(&SemanticToken::new_static("editor.text")),
            Some(&Color::srgb_u8(0xE8, 0xE8, 0xE8))
        );
    }
}
