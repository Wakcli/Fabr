//! The Inspector dock and the Node/History dock beneath it.

use eme::feathers::constants::fonts;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{FontSize, FontSourceTemplate, FontWeight, TextFont};

use super::chrome::*;
use super::docks;
use super::icons;
use crate::scene::SceneState;
use crate::tokens;

/// Which transform row an inspector field reflects.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    Position,
    Rotation,
    Scale,
}

/// Marks a transform value field so the scene can keep it live.
#[derive(Component, Clone, Copy)]
pub struct AxisValue(pub Prop, pub u8);

/// Marks the inspector header label so it shows the selected object.
#[derive(Component, Clone, Default)]
pub struct ObjectTitle;

pub fn right_dock() -> Part {
    let inspector = inspector_dock();
    let history = docks::history_dock();

    Box::new(bsn! {
        Node {
            width: px(RIGHT_DOCK_W),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [
            Node {
                flex_grow: 0.68,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {inspector} ]
            --
            {splitter(false)}
            --
            Node {
                flex_grow: 0.32,
                flex_basis: px(0),
                min_height: px(0),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            }
            Children [ {history} ]
        ]
    })
}

fn inspector_dock() -> Part {
    let tabs = vec![tab("Inspector", true)];
    let tail = vec![icon_button(icons::dock_menu(), 20.0)];

    let body = Box::new(bsn! {
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
            {object_row()}
            --
            {section("Transform", transform_rows())}
            --
            {section("Visibility", vec![visibility_row()])}
            --
            {section("Script", vec![script_row()])}
            --
            {spacer()}
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

fn object_row() -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            border: UiRect { bottom: px(1) },
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {icon_button(icons::lock(), 18.0)}
            --
            {icons::badge(icons::NODE)}
            --
            {title_text("Player".into())}
            --
            {spacer()}
            --
            {icon_button(icons::dock_menu(), 20.0)}
        ]
    })
}

fn title_text(value: String) -> Part {
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        ObjectTitle
    })
}

fn value_text(value: String, seat: AxisValue) -> Part {
    Box::new(bsn! {
        Text(value)
        TextFont {
            font: FontSourceTemplate::Handle(fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        seat
    })
}

fn section(title: &str, rows: Vec<Part>) -> Part {
    let mut kids = vec![Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(7)),
            column_gap: px(6),
        }
        ThemeBackgroundColor(tokens::CATEGORY)
        Children [
            {icons::caret(true)}
            --
            {text(title, FONT_SIZE, tokens::SECTION_TEXT)}
        ]
    }) as Part];
    kids.extend(rows);

    col(0.0, kids)
}

fn transform_rows() -> Vec<Part> {
    vec![
        property(
            "Position",
            Prop::Position,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        property(
            "Rotation",
            Prop::Rotation,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "0"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "0"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "0"),
            ],
        ),
        property(
            "Scale",
            Prop::Scale,
            [
                ("X", tokens::AXIS_X, tokens::AXIS_X_TEXT, "1"),
                ("Y", tokens::AXIS_Y, tokens::AXIS_Y_TEXT, "1"),
                ("Z", tokens::AXIS_Z, tokens::AXIS_Z_TEXT, "1"),
            ],
        ),
    ]
}

fn label_cell(label: &str) -> Part {
    Box::new(bsn! {
        Node { width: px(64) }
        Children [ {text(label, FONT_SIZE, tokens::PROP_TEXT)} ]
    })
}

fn property(label: &str, prop: Prop, axes: [(&str, ThemeToken, ThemeToken, &str); 3]) -> Part {
    let mut fields = Vec::new();
    for (i, (letter, line, letter_tint, value)) in axes.into_iter().enumerate() {
        fields.push(axis_field(
            letter,
            line,
            letter_tint,
            value,
            AxisValue(prop, i as u8),
        ));
    }

    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(4),
            },
            column_gap: px(6),
        }
        Children [
            {label_cell(label)}
            --
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(4),
            }
            Children [ {fields} ]
        ]
    })
}

fn axis_field(
    letter: &str,
    line: ThemeToken,
    letter_tint: ThemeToken,
    value: &str,
    seat: AxisValue,
) -> Part {
    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(FIELD_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            border: UiRect { bottom: px(2) },
            border_radius: px(3),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::FIELD)
        ThemeBorderColor(line)
        Children [
            Node {
                width: px(24),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            }
            ThemeBackgroundColor(tokens::LABEL_STRIP)
            Children [ {text(letter, FONT_SIZE, letter_tint)} ]
            --
            Node {
                flex_grow: 1.0,
                display: Display::Flex,
                align_items: AlignItems::Center,
                padding: UiRect { left: px(6) },
            }
            Children [ {value_text(value.into(), seat)} ]
        ]
    })
}

fn visibility_row() -> Part {
    Box::new(bsn! {
        Node {
            height: px(PROPERTY_ROW_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
            column_gap: px(6),
        }
        Children [
            {label_cell("Visible")}
            --
            {spacer()}
            --
            {icons::check()}
        ]
    })
}

fn script_row() -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(5),
            },
            column_gap: px(6),
        }
        Children [
            {label_cell("Script")}
            --
            {field(
                vec![
                    icons::file(),
                    text("Empty", FONT_SIZE, tokens::TEXT_DIM),
                ],
                FIELD_H,
            )}
        ]
    })
}

fn fmt_num(value: f32) -> String {
    let mut text = format!("{value:.2}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

fn degrees(rotation: [f32; 4]) -> [f32; 3] {
    let (x, y, z) = Quat::from_array(rotation).to_euler(EulerRot::XYZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

/// Keeps the inspector header and transform fields in step with the selection.
pub fn sync_inspector(
    state: Res<SceneState>,
    mut values: Query<(&AxisValue, &mut Text), Without<ObjectTitle>>,
    mut titles: Query<&mut Text, (With<ObjectTitle>, Without<AxisValue>)>,
) {
    let selected = state.selected_object();

    for (seat, mut value) in &mut values {
        let text = match (selected, seat.0) {
            (Some(obj), Prop::Position) => fmt_num(obj.translation[seat.1 as usize]),
            (Some(obj), Prop::Rotation) => fmt_num(degrees(obj.rotation)[seat.1 as usize]),
            (Some(obj), Prop::Scale) => fmt_num(obj.scale[seat.1 as usize]),
            (None, Prop::Scale) => "1".into(),
            (None, _) => "0".into(),
        };
        if value.0 != text {
            value.0 = text;
        }
    }

    let title = selected.map_or("No selection", |obj| obj.name.as_str());
    for mut text in &mut titles {
        if text.0 != title {
            text.0 = title.into();
        }
    }
}
