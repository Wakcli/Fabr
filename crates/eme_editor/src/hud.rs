//! Game-view HUD drawn by scripts through `fabr.ui`.

use eme::prelude::*;
use eme::text::{FontSize, TextColor, TextFont};
use eme::ui::widget::Text;
use eme::window::{PrimaryWindow, Window};
use eme_script::{ScriptUi, UiElement};
use crate::orbit::ViewMode;
use crate::play::PlayState;

/// Marks one entity spawned for the current HUD.
#[derive(Component, Clone, Copy, Default)]
pub struct HudElement;

pub fn install(app: &mut App) {
    app.add_systems(Update, (sync, clicks));
}

fn visible(play: &PlayState, view: &ViewMode) -> bool {
    play.running() && *view == ViewMode::Game
}

fn sync(
    ui: Res<ScriptUi>,
    play: Res<PlayState>,
    view: Res<ViewMode>,
    mut commands: Commands,
    roots: Query<Entity, With<HudElement>>,
    mut last: Local<bool>,
) {
    let shown = visible(&play, &view);
    if !ui.is_changed() && *last == shown {
        return;
    }
    *last = shown;
    for entity in &roots {
        commands.entity(entity).despawn();
    }
    if !shown {
        return;
    }
    for element in &ui.elements {
        match element {
            UiElement::Panel { x, y, w, h, color } => {
                commands.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(x * 100.0),
                        top: percent(y * 100.0),
                        width: percent(w * 100.0),
                        height: percent(h * 100.0),
                        ..default()
                    },
                    BackgroundColor(Color::linear_rgba(color[0], color[1], color[2], color[3])),
                    HudElement,
                ));
            }
            UiElement::Text { x, y, size, text, color } => {
                commands.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(x * 100.0),
                        top: percent(y * 100.0),
                        ..default()
                    },
                    Text(text.clone()),
                    TextFont { font_size: FontSize::Px(*size), ..default() },
                    TextColor(Color::linear_rgb(color[0], color[1], color[2])),
                    HudElement,
                ));
            }
            UiElement::Button { x, y, w, h, label, .. } => {
                let button = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(x * 100.0),
                            top: percent(y * 100.0),
                            width: percent(w * 100.0),
                            height: percent(h * 100.0),
                            display: Display::Flex,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::linear_rgba(0.16, 0.18, 0.22, 0.95)),
                        HudElement,
                    ))
                    .id();
                commands.spawn((
                    Text(label.clone()),
                    TextFont { font_size: FontSize::Px(18.0), ..default() },
                    TextColor(Color::WHITE),
                    ChildOf(button),
                ));
            }
        }
    }
}

fn clicks(
    mut ui: ResMut<ScriptUi>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let size = window.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    if let Some(id) = ui.button_at(cursor.x / size.x, cursor.y / size.y) {
        ui.clicks.push(id);
    }
}
