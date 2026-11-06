//! Chrome primitives shared by the editor frame: rows, tabs, separators, and text.

use eme::feathers::{
    constants::fonts,
    theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken},
};
use eme::prelude::*;
use eme::scene::{bsn, SceneList};
use eme::text::{FontSize, FontSourceTemplate, FontWeight, TextFont};
use eme::ui::widget::Text;

use crate::tokens;

/// A scene fragment that can be nested anywhere in the frame.
pub type Part = Box<dyn SceneList>;

/// The editor body size.
pub const FONT_SIZE: f32 = 15.0;
/// The log body size.
pub const FONT_SIZE_OUTPUT: f32 = 13.0;

pub const MENU_BAR_H: f32 = 30.0;
pub const SCENE_TAB_H: f32 = 29.0;
pub const DOCK_TAB_H: f32 = 29.0;
pub const TOOL_ROW_H: f32 = 28.0;
pub const STATUS_H: f32 = 27.0;
pub const TREE_ROW_H: f32 = 21.0;
pub const PROPERTY_ROW_H: f32 = 27.0;
pub const FIELD_H: f32 = 27.0;

pub fn text(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    let value = value.into();
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(token)
    })
}

pub fn text_bold(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    let value = value.into();
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::BOLD),
            font_size: FontSize::Px(size),
            weight: FontWeight::BOLD,
        }
        ThemeTextColor(token)
    })
}

/// The log body, set in the bundled mono face.
pub fn text_mono(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    let value = value.into();
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::MONO),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(token)
    })
}

pub fn col(gap: f32, kids: Vec<Part>) -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(gap),
        }
        Children [ {kids} ]
    })
}

pub fn spacer() -> Part {
    Box::new(bsn! { Node { flex_grow: 1.0 } })
}

/// A fixed gap between two controls.
pub fn hgap(width: f32) -> Part {
    Box::new(bsn! { Node { width: px(width) } })
}

pub fn vline(height: f32, token: ThemeToken) -> Part {
    Box::new(bsn! {
        Node { width: px(1), height: px(height) }
        ThemeBackgroundColor(token)
    })
}

pub fn rect(width: f32, height: f32, token: ThemeToken, radius: f32) -> Part {
    Box::new(bsn! {
        Node {
            width: px(width),
            height: px(height),
            border_radius: px(radius),
        }
        ThemeBackgroundColor(token)
    })
}

/// A flat toolbar button that holds an icon.
pub fn icon_button(icon: Part, size: f32) -> Part {
    Box::new(bsn! {
        Node {
            width: px(size),
            height: px(size),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Children [ {icon} ]
    })
}

fn tab_shell(label: &str, bg: ThemeToken, fg: ThemeToken, border: f32, padding: UiRect) -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            height: percent(100),
            padding: {padding},
            border: UiRect { bottom: px(border) },
            column_gap: px(6),
        }
        ThemeBackgroundColor(bg)
        ThemeBorderColor(tokens::TAB_HIGHLIGHT)
        Children [ {text(label, FONT_SIZE, fg)} ]
    })
}

/// A side dock tab: unselected tabs sit on the dark strip, the selected one
/// takes the dock body color and a two pixel accent underline.
pub fn tab(label: &str, active: bool) -> Part {
    let (bg, fg, border) = if active {
        (tokens::PANEL, tokens::TEXT, 2.0)
    } else {
        (tokens::WINDOW, tokens::TEXT_DIM, 0.0)
    };
    tab_shell(
        label,
        bg,
        fg,
        border,
        UiRect {
            left: px(11),
            right: px(11),
            top: px(5),
            bottom: px(5),
        },
    )
}

/// The tab strip across the top of a dock, with its dock menu at the far right.
pub fn tab_strip(tabs: Vec<Part>, tail: Vec<Part>, bg: ThemeToken) -> Part {
    let mut kids = tabs;
    kids.push(spacer());
    kids.extend(tail);

    Box::new(bsn! {
        Node {
            height: px(DOCK_TAB_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            padding: UiRect { right: px(4) },
        }
        ThemeBackgroundColor(bg)
        Children [ {kids} ]
    })
}

pub fn menu_item(label: &str) -> Part {
    Box::new(bsn! {
        Node {
            height: px(27),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            border_radius: px(3),
        }
        Children [ {text(label, FONT_SIZE, tokens::TEXT)} ]
    })
}

/// A sunken field: dark fill with a two pixel underline.
pub fn field(kids: Vec<Part>, height: f32) -> Part {
    Box::new(bsn! {
        Node {
            height: px(height),
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            border: UiRect { bottom: px(2) },
            column_gap: px(6),
            border_radius: px(3),
        }
        ThemeBackgroundColor(tokens::FIELD)
        ThemeBorderColor(tokens::FIELD_LINE)
        Children [ {kids} ]
    })
}
