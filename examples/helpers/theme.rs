use eme::{
    color::palettes, feathers::theme::ThemeProps, platform::collections::HashMap, prelude::*,
};

/// Creates a basic example theme props for the radio buttons and number inputs.
pub fn basic_example_theme(text_color: Color) -> ThemeProps {
    let mut color = HashMap::new();

    // Radio Button tokens
    color.insert(eme::feathers::tokens::RADIO_TEXT, text_color);
    color.insert(eme::feathers::tokens::RADIO_MARK, Color::BLACK);
    color.insert(eme::feathers::tokens::RADIO_MARK_HOVER, Color::BLACK);
    color.insert(eme::feathers::tokens::RADIO_MARK_PRESSED, Color::BLACK);

    color.insert(eme::feathers::tokens::RADIO_BG, Color::WHITE);
    color.insert(
        eme::feathers::tokens::RADIO_BG_HOVER,
        palettes::basic::GRAY.into(),
    );
    color.insert(
        eme::feathers::tokens::RADIO_BG_PRESSED,
        palettes::basic::GRAY.into(),
    );
    color.insert(eme::feathers::tokens::RADIO_BG_CHECKED, Color::WHITE);
    color.insert(eme::feathers::tokens::RADIO_BG_CHECKED_HOVER, Color::BLACK);
    color.insert(
        eme::feathers::tokens::RADIO_BG_CHECKED_PRESSED,
        Color::BLACK,
    );

    color.insert(eme::feathers::tokens::RADIO_BORDER, Color::BLACK);
    color.insert(
        eme::feathers::tokens::RADIO_BORDER_HOVER,
        palettes::basic::GRAY.into(),
    );
    color.insert(
        eme::feathers::tokens::RADIO_BORDER_PRESSED,
        palettes::basic::BLACK.into(),
    );
    color.insert(eme::feathers::tokens::RADIO_BORDER_CHECKED, Color::BLACK);
    color.insert(
        eme::feathers::tokens::RADIO_BORDER_CHECKED_HOVER,
        Color::BLACK,
    );
    color.insert(
        eme::feathers::tokens::RADIO_BORDER_CHECKED_PRESSED,
        Color::BLACK,
    );

    // Number Input tokens
    color.insert(eme::feathers::tokens::TEXT_INPUT_TEXT, text_color);
    color.insert(eme::feathers::tokens::TEXT_INPUT_BG, Color::BLACK);
    color.insert(eme::feathers::tokens::TEXT_INPUT_LABEL_BG, Color::BLACK);

    color.insert(eme::feathers::tokens::SLIDER_BAR, Color::WHITE);
    color.insert(eme::feathers::tokens::SLIDER_BAR_HOVER, Color::WHITE);
    color.insert(eme::feathers::tokens::SLIDER_BAR_PRESSED, Color::WHITE);
    color.insert(eme::feathers::tokens::SLIDER_BG, Color::BLACK);
    color.insert(eme::feathers::tokens::SLIDER_BG_HOVER, Color::BLACK);
    color.insert(eme::feathers::tokens::SLIDER_BG_PRESSED, Color::BLACK);

    color.insert(
        eme::feathers::tokens::TEXT_INPUT_CURSOR,
        eme::feathers::palette::ACCENT.lighter(0.2),
    );
    color.insert(
        eme::feathers::tokens::TEXT_INPUT_SELECTION,
        eme::feathers::palette::ACCENT,
    );
    color.insert(
        eme::feathers::tokens::TEXT_INPUT_SELECTION_UNFOCUSED,
        eme::feathers::palette::TRANSPARENT,
    );

    // Checkbox tokens
    color.insert(eme::feathers::tokens::CHECKBOX_TEXT, text_color);
    color.insert(eme::feathers::tokens::CHECKBOX_BG, Color::BLACK);
    color.insert(eme::feathers::tokens::CHECKBOX_MARK, Color::WHITE);
    color.insert(eme::feathers::tokens::CHECKBOX_BORDER, Color::BLACK);
    color.insert(
        eme::feathers::tokens::CHECKBOX_BORDER_HOVER,
        eme::feathers::palette::GRAY_2,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BORDER_CHECKED_HOVER,
        eme::feathers::palette::GRAY_0,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BORDER_PRESSED,
        eme::feathers::palette::GRAY_1,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BORDER_CHECKED_PRESSED,
        eme::feathers::palette::GRAY_1,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BORDER_CHECKED,
        Color::BLACK,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BG_HOVER,
        eme::feathers::palette::GRAY_0,
    );
    color.insert(eme::feathers::tokens::CHECKBOX_BG_CHECKED, Color::BLACK);
    color.insert(
        eme::feathers::tokens::CHECKBOX_BG_CHECKED_HOVER,
        Color::BLACK,
    );
    color.insert(
        eme::feathers::tokens::CHECKBOX_BG_CHECKED_PRESSED,
        eme::feathers::palette::GRAY_1,
    );
    color.insert(eme::feathers::tokens::CHECKBOX_BG_PRESSED, Color::BLACK);
    // Feathers Button / Select
    color.insert(
        eme::feathers::tokens::BUTTON_BG,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::BUTTON_BG_HOVER,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::BUTTON_BG_PRESSED,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::MENU_BG,
        eme::feathers::palette::GRAY_0,
    );
    color.insert(
        eme::feathers::tokens::MENU_BORDER,
        eme::feathers::palette::GRAY_0,
    );
    color.insert(
        eme::feathers::tokens::MENUITEM_BG_HOVER,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::MENUITEM_BG_PRESSED,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::MENUITEM_BG_FOCUSED,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::LISTROW_BG,
        eme::feathers::palette::TRANSPARENT,
    );
    color.insert(
        eme::feathers::tokens::LISTROW_BG_HOVER,
        eme::feathers::palette::BLACK,
    );
    color.insert(
        eme::feathers::tokens::LISTROW_BG_SELECTED,
        eme::feathers::palette::BLACK,
    );
    color.insert(eme::feathers::tokens::BUTTON_TEXT, text_color);
    color.insert(eme::feathers::tokens::LISTROW_TEXT, text_color);

    // Slider tokens
    color.insert(
        eme::feathers::tokens::SLIDER_BG,
        eme::feathers::palette::BLACK,
    );
    color.insert(
        eme::feathers::tokens::SLIDER_BG_HOVER,
        eme::feathers::palette::GRAY_0,
    );
    color.insert(
        eme::feathers::tokens::SLIDER_BG_PRESSED,
        eme::feathers::palette::GRAY_1,
    );
    color.insert(
        eme::feathers::tokens::SLIDER_BAR,
        eme::feathers::palette::ACCENT,
    );
    color.insert(
        eme::feathers::tokens::SLIDER_BAR_HOVER,
        eme::feathers::palette::ACCENT.lighter(0.05),
    );
    color.insert(
        eme::feathers::tokens::SLIDER_BAR_PRESSED,
        eme::feathers::palette::ACCENT.lighter(0.1),
    );
    color.insert(eme::feathers::tokens::SLIDER_TEXT, text_color);

    // Main text color
    color.insert(eme::feathers::tokens::TEXT_MAIN, text_color);
    ThemeProps::new_non_contextual(color)
}
