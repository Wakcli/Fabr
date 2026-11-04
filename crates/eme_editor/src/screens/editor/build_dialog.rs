//! The Build tab: target readout, output paths, and a live build log.

use std::process::Command;

use eme::color::Alpha;
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::picking::hover::Hovered;
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui::InteractionDisabled;
use eme::ui_widgets::{Activate, Button};

use super::chrome::*;
use super::icons;
use crate::build::{self, BuildState, Level, Target};
use crate::persist::SCENE_PATH;
use crate::scene::SceneState;
use crate::tokens;

const PANEL_W: f32 = 560.0;
const PANEL_H: f32 = 440.0;
const LABEL_W: f32 = 104.0;
const LOG_SIZE: f32 = 12.0;
const LOG_LINES: usize = 10;

/// Root of the modal layer; the sync system swaps it when the state changes.
#[derive(Component, Clone, Copy, Default)]
pub struct BuildRoot;

/// What a build dialog button does when it is activated.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Verb {
    #[default]
    Close,
    Start,
    Run,
}

/// The action a dialog button carries.
#[derive(Component, Clone, Copy, Default)]
pub struct Action(pub Verb);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    Field,
    Accent,
}

impl Fill {
    fn base(self) -> ThemeToken {
        match self {
            Fill::Field => tokens::FIELD,
            Fill::Accent => tokens::ACCENT,
        }
    }

    fn hover(self) -> ThemeToken {
        match self {
            Fill::Field => tokens::SELECT,
            Fill::Accent => tokens::ACCENT,
        }
    }
}

/// The menu-bar button that opens the dialog.
pub fn top_bar_button() -> Part {
    Box::new(bsn! {
        Node {
            height: px(22),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(11)),
            border_radius: px(3),
        }
        ThemeBackgroundColor(tokens::ACCENT)
        Button
        Children [ {text_bold("Build", FONT_SIZE_OUTPUT, tokens::WINDOW)} ]
        on(open_modal)
    })
}

fn open_modal(_: On<Activate>, mut state: ResMut<BuildState>) {
    state.open();
}

/// Rebuilds the modal whenever the state changes.
pub fn sync_modal(
    mut commands: Commands,
    state: Res<BuildState>,
    roots: Query<Entity, With<BuildRoot>>,
    mut last: Local<Option<u64>>,
) {
    if *last == Some(state.rev) {
        return;
    }
    *last = Some(state.rev);
    for root in &roots {
        commands.entity(root).despawn();
    }
    if !state.open {
        return;
    }
    let root = commands.spawn_scene(layer()).id();
    commands.entity(root).insert(BuildRoot);
    let scrim = commands.spawn_scene(scrim()).id();
    commands.entity(scrim).insert(ChildOf(root));
    let panel = commands.spawn_scene(panel(&state)).id();
    commands.entity(panel).insert(ChildOf(root));
}

fn layer() -> impl Scene {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
        }
        GlobalZIndex(200)
        Pickable::IGNORE
    })
}

fn scrim() -> impl Scene {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
        }
        ThemeBackgroundColor(tokens::SCRIM)
        Button
        on(dismiss)
    })
}

fn dismiss(_: On<Activate>, mut state: ResMut<BuildState>) {
    state.close();
}

fn panel(state: &BuildState) -> impl Scene {
    let kids = vec![
        header(),
        divider(),
        target_section(),
        value_row("Output folder", build::OUT_DIR),
        value_row("Main scene", SCENE_PATH),
        log_section(state),
        footer(state),
    ];

    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            margin: UiRect {
                left: px(-PANEL_W / 2.0),
                top: px(-PANEL_H / 2.0),
            },
            width: px(PANEL_W),
            height: px(PANEL_H),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(10),
            padding: px(16),
            border: px(1),
            border_radius: px(6),
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        OverrideClip
        BoxShadow::new(Color::BLACK.with_alpha(0.6), px(0), px(0), px(3), px(14))
        Children [ {kids} ]
    })
}

fn header() -> Part {
    let close = Box::new(bsn! {
        Node {
            width: px(22),
            height: px(22),
            display: Display::Flex,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: px(3),
        }
        Button
        Action(Verb::Close)
        Children [ {icons::close()} ]
        on(activate)
    });

    Box::new(bsn! {
        Node {
            height: px(28),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10),
        }
        Children [
            {text_bold("Build", FONT_SIZE + 3.0, tokens::TEXT)}
            --
            {text(
                "package this scene and the runtime player into a game folder",
                FONT_SIZE_OUTPUT,
                tokens::TEXT_DIM,
            )}
            --
            {spacer()}
            --
            {close}
        ]
    })
}

fn divider() -> Part {
    Box::new(bsn! {
        Node { height: px(1) }
        ThemeBackgroundColor(tokens::BORDER)
    })
}

fn label(value: &str) -> Part {
    Box::new(bsn! {
        Node { width: px(LABEL_W) }
        Children [ {text(value, FONT_SIZE_OUTPUT, tokens::TEXT_DIM)} ]
    })
}

fn row(label_text: &str, kids: Vec<Part>) -> Part {
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10),
            min_height: px(26),
        }
        Children [
            {label(label_text)}
            --
            {kids}
        ]
    })
}

fn value_row(label_text: &str, value: &str) -> Part {
    row(
        label_text,
        vec![text_mono(value, FONT_SIZE_OUTPUT, tokens::TEXT)],
    )
}

fn note_line(value: &str) -> Part {
    Box::new(bsn! {
        Node { margin: UiRect { left: px(LABEL_W + 10.0) } }
        Children [ {text(value, LOG_SIZE, tokens::TEXT_DIM)} ]
    })
}

fn target_section() -> Part {
    let mut chips: Vec<Part> = Vec::new();
    let mut refused: Vec<&'static str> = Vec::new();
    for target in Target::ALL {
        if !chips.is_empty() {
            chips.push(hgap(6.0));
        }
        chips.push(chip(target.label(), target.ready()));
        if !target.ready() {
            refused.push(target.label());
        }
    }
    let note = format!("{} need a CI template", refused.join(" and "));

    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
        }
        Children [
            {row("Target", chips)}
            --
            {note_line(&note)}
        ]
    })
}

fn chip(name: &str, ready: bool) -> Part {
    let (bg, fg) = if ready {
        (tokens::ACCENT, tokens::WINDOW)
    } else {
        (tokens::FIELD, tokens::TEXT_DIM)
    };
    let label = text_bold(name, FONT_SIZE_OUTPUT, fg);

    if ready {
        Box::new(bsn! {
            Node {
                height: px(24),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(12)),
                border_radius: px(3),
            }
            ThemeBackgroundColor(bg)
            Button
            Children [ {label} ]
        })
    } else {
        Box::new(bsn! {
            Node {
                height: px(24),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(12)),
                border_radius: px(3),
            }
            ThemeBackgroundColor(bg)
            Button
            InteractionDisabled
            Children [ {label} ]
        })
    }
}

fn log_section(state: &BuildState) -> Part {
    let mut kids: Vec<Part> = Vec::new();
    if state.log.is_empty() {
        kids.push(text("Log output appears here.", LOG_SIZE, tokens::TEXT_DIM));
    } else {
        let start = state.log.len().saturating_sub(LOG_LINES);
        for line in &state.log[start..] {
            kids.push(text_mono(
                line.text.clone(),
                LOG_SIZE,
                level_token(line.level),
            ));
        }
    }

    Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            min_height: px(0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(2),
            padding: px(8),
            border: px(1),
            border_radius: px(4),
            overflow: Overflow::clip(),
        }
        ThemeBackgroundColor(tokens::INSET)
        ThemeBorderColor(tokens::BORDER)
        Children [ {kids} ]
    })
}

fn level_token(level: Level) -> ThemeToken {
    match level {
        Level::Step => tokens::TEXT_MEDIUM,
        Level::Done => tokens::TEXT,
        Level::Error => tokens::AXIS_X,
    }
}

fn footer(state: &BuildState) -> Part {
    let buttons = vec![
        text(state.status(), FONT_SIZE_OUTPUT, tokens::TEXT_MEDIUM),
        spacer(),
        button(
            "Run",
            Verb::Run,
            Fill::Field,
            state.game.is_some() && !state.running,
        ),
        button("Build", Verb::Start, Fill::Accent, !state.running),
        button("Close", Verb::Close, Fill::Field, true),
    ];

    Box::new(bsn! {
        Node {
            height: px(30),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        Children [ {buttons} ]
    })
}

fn button(name: &str, verb: Verb, fill: Fill, enabled: bool) -> Part {
    let fg = if !enabled {
        tokens::TEXT_DIM
    } else if fill == Fill::Accent {
        tokens::WINDOW
    } else {
        tokens::TEXT
    };
    let label = text_bold(name, FONT_SIZE_OUTPUT, fg);
    let bg = fill.base();

    if enabled {
        Box::new(bsn! {
            Node {
                height: px(28),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(14)),
                border_radius: px(4),
            }
            ThemeBackgroundColor(bg)
            Button
            Hovered
            Action(verb)
            Children [ {label} ]
            on(activate)
        })
    } else {
        Box::new(bsn! {
            Node {
                height: px(28),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(14)),
                border_radius: px(4),
            }
            ThemeBackgroundColor(tokens::FIELD)
            Button
            InteractionDisabled
            Children [ {label} ]
        })
    }
}

fn activate(
    activate: On<Activate>,
    actions: Query<&Action>,
    mut state: ResMut<BuildState>,
    scene: Res<SceneState>,
    scenes: Res<crate::tabs::OpenScenes>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match action.0 {
        Verb::Close => state.close(),
        Verb::Start => state.start(scene.doc.clone(), scenes.path().to_string()),
        Verb::Run => run_game(&mut state),
    }
}

fn run_game(state: &mut BuildState) {
    let Some(game) = state.game.clone() else {
        return;
    };
    match Command::new(&game).current_dir(build::OUT_DIR).spawn() {
        Ok(child) => state.line(
            Level::Done,
            format!("launched {} (pid {})", game.display(), child.id()),
        ),
        Err(err) => state.line(
            Level::Error,
            format!("could not launch {}: {err}", game.display()),
        ),
    }
}

/// Highlights hovered dialog buttons.
pub fn style_buttons(
    mut commands: Commands,
    buttons: Query<
        (Entity, &Action, &Hovered, &ThemeBackgroundColor),
        (With<Button>, Without<InteractionDisabled>),
    >,
) {
    for (ent, action, hovered, current) in &buttons {
        let fill = match action.0 {
            Verb::Start => Fill::Accent,
            Verb::Run | Verb::Close => Fill::Field,
        };
        let want = if hovered.0 { fill.hover() } else { fill.base() };
        if current.0 != want {
            commands.entity(ent).insert(ThemeBackgroundColor(want));
        }
    }
}

/// Escape closes the dialog.
pub fn escape(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<BuildState>) {
    if state.open && keys.just_pressed(KeyCode::Escape) {
        state.close();
    }
}

/// Drops the dialog when the editor screen goes away.
pub fn close_on_teardown(
    panels: Query<(), With<super::viewport::ViewportPanel>>,
    mut state: ResMut<BuildState>,
) {
    if panels.is_empty() {
        state.close();
    }
}
