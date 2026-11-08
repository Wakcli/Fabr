//! Animation Editor: clip playback, speed, looping and timeline scrubbing.

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeTextColor};
use eme::input::keyboard::KeyboardInput;
use eme::input_focus::tab_navigation::TabIndex;
use eme::input_focus::{FocusCause, FocusedInput, InputFocus};
use eme::prelude::*;
use eme::scene::bsn;
use eme::text::{
    EditableText, FontSize, FontSourceTemplate, FontWeight, LineBreak, TextFont, TextLayout,
};
use eme::ui_widgets::{Activate, Button, TextInput};
use eme_anim::{AnimationClock, AnimationSet};
use eme_game_scene::AnimationRef;

use crate::scene::SceneState;
use crate::screens::editor::chrome::{
    field, spacer, text, text_bold, Part, FIELD_H, FONT_SIZE,
};
use crate::tokens;
use crate::undo::Undo;

/// Animation Editor window state.
#[derive(Resource, Default)]
pub struct AnimationState {
    /// Whether the window is visible.
    pub open: bool,
    /// Speed draft, seeded when the window opens.
    pub speed: String,
    /// Scrub draft in seconds, seeded when the window opens.
    pub scrub: String,
    /// Last message.
    pub status: String,
    /// Bumped to rebuild the panel.
    pub rev: u64,
}

impl AnimationState {
    /// Opens the window with drafts from the current selection.
    pub fn open_with(&mut self, set: Option<&AnimationSet>) {
        self.speed = set.map_or_else(|| "1".into(), |set| format!("{:.2}", set.speed));
        self.scrub = "0".into();
        self.status.clear();
        self.open = true;
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Root of the floating window.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimationRoot;

/// Host for the rebuilt rows.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimationBody;

/// Clip button in the animation window.
#[derive(Component, Clone, Default)]
pub struct AnimClipButton(pub String);

/// Stop button.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimStop;

/// Loop toggle.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimLoopToggle;

/// Speed input.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimSpeedField;

/// Scrub input.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimScrubField;

/// Bar showing playback progress.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimProgressFill;

/// Text showing `time / duration`.
#[derive(Component, Clone, Copy, Default)]
pub struct AnimTimeText;

pub fn install(app: &mut App) {
    app.init_resource::<AnimationState>()
        .add_systems(Update, (sync, tick));
}

pub fn panel() -> Part {
    let body = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(3),
            padding: px(6),
        }
        AnimationBody
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(520),
            top: px(110),
            width: px(470),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        AnimationRoot
        GlobalZIndex(339)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {body} ]
    })
}

fn selection(scene: &SceneState) -> Option<(u64, Entity)> {
    let id = scene.primary()?;
    let entity = *scene.entities.get(&id)?;
    Some((id, entity))
}

fn sync(
    window: Res<AnimationState>,
    scene: Res<SceneState>,
    sets: Query<&AnimationSet>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<AnimationRoot>>,
    hosts: Query<Entity, With<AnimationBody>>,
    mut shown: Local<Option<(Entity, u64, bool, Option<u64>, String, bool, bool)>>,
) {
    for mut root in &mut roots {
        let want = if window.open {
            Display::Flex
        } else {
            Display::None
        };
        if root.display != want {
            root.display = want;
        }
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    let selected = selection(&scene);
    let set = selected.and_then(|(_, entity)| sets.get(entity).ok());
    let key = (
        host,
        window.rev,
        window.open,
        selected.map(|(id, _)| id),
        set.map_or_else(String::new, |set| {
            format!(
                "{}|{:?}|{}|{:.3}|{}",
                set.clips.join(","),
                set.selected,
                set.playing,
                set.speed,
                set.looping
            )
        }),
        false,
        false,
    );
    if shown.as_ref() == Some(&key) {
        return;
    }
    *shown = Some(key);
    commands.entity(host).despawn_children();
    if !window.open {
        return;
    }

    let title = text_bold("Animation", FONT_SIZE + 1.0, tokens::TEXT);
    let hint = text("clip playback for the selected model", FONT_SIZE, tokens::TEXT_DIM);
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            height: px(26),
            padding: UiRect::horizontal(px(4)),
        }
        ChildOf(host)
        Children [ {title} -- {hint} -- {spacer()} ]
    });

    let (Some((_id, _)), Some(set)) = (selected, set) else {
        let line = text(
            if selected.is_none() {
                "Select a model in the Hierarchy"
            } else {
                "The selected object has no animation clips"
            },
            FONT_SIZE,
            tokens::TEXT_DIM,
        );
        commands.spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(4)), display: Display::Flex, align_items: AlignItems::Center }
            ChildOf(host)
            Children [ {line} ]
        });
        return;
    };
    for clip in &set.clips {
        let active = set.selected.as_deref() == Some(clip.as_str()) && set.playing;
        let (bg, fg) = if active {
            (tokens::SELECT, tokens::SELECT_TEXT)
        } else {
            (tokens::CLEAR, tokens::TEXT)
        };
        let label = text(clip.clone(), FONT_SIZE, fg);
        let name = clip.clone();
        commands
            .spawn_scene(bsn! {
                Node {
                    height: px(22),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(6)),
                }
                Button
                ThemeBackgroundColor(bg)
                AnimClipButton(name)
                ChildOf(host)
                Children [ {label} ]
            })
            .observe(play_clip);
    }

    let stop = text("Stop", FONT_SIZE, tokens::TEXT);
    let loop_label = if set.looping { "Loop: on" } else { "Loop: off" };
    let loop_text = text(loop_label, FONT_SIZE, tokens::TEXT);
    let actions = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
                height: px(24),
                padding: UiRect::horizontal(px(4)),
            }
            ChildOf(host)
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            AnimStop
            ChildOf(actions)
            Children [ {stop} ]
        })
        .observe(stop_clip);
    commands
        .spawn_scene(bsn! {
            Node { height: px(22), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            AnimLoopToggle
            ChildOf(actions)
            Children [ {loop_text} ]
        })
        .observe(toggle_loop);

    let bar = commands
        .spawn_scene(bsn! {
            Node {
                width: percent(100),
                height: px(6),
                border_radius: px(3),
            }
            ThemeBackgroundColor(tokens::FIELD)
            ChildOf(host)
        })
        .id();
    commands.spawn_scene(bsn! {
        Node {
            width: percent(0),
            height: px(6),
            border_radius: px(3),
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
        }
        ThemeBackgroundColor(tokens::ACCENT)
        AnimProgressFill
        ChildOf(bar)
    });
    let initial_time = "0.00 / 0.00 s".to_string();
    let time_label = Box::new(bsn! {
        Text(initial_time)
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT_DIM)
        AnimTimeText
    }) as Part;
    commands.spawn_scene(bsn! {
        Node { height: px(18), padding: UiRect::horizontal(px(4)), display: Display::Flex, align_items: AlignItems::Center }
        ChildOf(host)
        Children [ {time_label} ]
    });

    let speed_value = window.speed.clone();
    let scrub_value = window.scrub.clone();
    let speed_input = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(20),
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(speed_value)
        TextInput
        TabIndex(0)
        AnimSpeedField
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        on(focus_speed)
        on(speed_key)
    });
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
                height: px(30),
                padding: UiRect::horizontal(px(4)),
            }
            ChildOf(host)
            Children [ {text("Speed", FONT_SIZE, tokens::TEXT_DIM)} -- {field(vec![speed_input], FIELD_H)} ]
        });

    let scrub_input = Box::new(bsn! {
        Node {
            flex_grow: 1.0,
            height: px(20),
            display: Display::Flex,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
        }
        EditableText::new(scrub_value)
        TextInput
        TabIndex(0)
        AnimScrubField
        TextLayout { linebreak: LineBreak::NoWrap }
        TextFont {
            font: FontSourceTemplate::Handle(eme::feathers::constants::fonts::REGULAR),
            font_size: FontSize::Px(FONT_SIZE),
            weight: FontWeight::NORMAL,
        }
        ThemeTextColor(tokens::TEXT)
        on(focus_scrub)
        on(scrub_key)
    });
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            height: px(30),
            padding: UiRect::horizontal(px(4)),
        }
        ChildOf(host)
        Children [ {text("Scrub (s)", FONT_SIZE, tokens::TEXT_DIM)} -- {field(vec![scrub_input], FIELD_H)} ]
    });

    if !window.status.is_empty() {
        let status = text(window.status.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(4)) }
            ChildOf(host)
            Children [ {status} ]
        });
    }
}

fn focus_speed(
    press: On<PointerPress>,
    fields: Query<(), With<AnimSpeedField>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

fn focus_scrub(
    press: On<PointerPress>,
    fields: Query<(), With<AnimScrubField>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !fields.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

fn speed_key(
    key: On<FocusedInput<KeyboardInput>>,
    fields: Query<&EditableText, With<AnimSpeedField>>,
    mut window: ResMut<AnimationState>,
    mut scene: ResMut<SceneState>,
    mut sets: Query<&mut AnimationSet>,
    mut undo: ResMut<Undo>,
) {
    if key.input.key_code != KeyCode::Enter || !key.input.state.is_pressed() {
        return;
    }
    let Ok(input) = fields.get(key.focused_entity) else {
        return;
    };
    let Ok(speed) = input.value().to_string().trim().parse::<f32>() else {
        window.status = "speed must be a number".into();
        window.rev = window.rev.wrapping_add(1);
        return;
    };
    let Some((id, entity)) = selection(&scene) else {
        return;
    };
    let Ok(mut set) = sets.get_mut(entity) else {
        return;
    };
    undo.checkpoint(&scene, "Animation Speed");
    set.set_speed(speed);
    window.status = format!("speed {:.2}", set.speed);
    window.speed = format!("{:.2}", set.speed);
    store(&mut scene, id, &set);
    window.rev = window.rev.wrapping_add(1);
}

fn scrub_key(
    key: On<FocusedInput<KeyboardInput>>,
    fields: Query<&EditableText, With<AnimScrubField>>,
    mut window: ResMut<AnimationState>,
    scene: Res<SceneState>,
    mut sets: Query<&mut AnimationSet>,
    clocks: Query<&AnimationClock>,
) {
    if key.input.key_code != KeyCode::Enter || !key.input.state.is_pressed() {
        return;
    }
    let Ok(input) = fields.get(key.focused_entity) else {
        return;
    };
    let Ok(time) = input.value().to_string().trim().parse::<f32>() else {
        window.status = "scrub needs a time in seconds".into();
        window.rev = window.rev.wrapping_add(1);
        return;
    };
    let Some((_, entity)) = selection(&scene) else {
        return;
    };
    let Ok(mut set) = sets.get_mut(entity) else {
        return;
    };
    let duration = clocks.get(entity).map_or(0.0, |clock| clock.duration);
    set.seek_to(time, duration);
    window.status = format!("seek {:.2} s", set.seek.unwrap_or(0.0));
    window.rev = window.rev.wrapping_add(1);
}

fn play_clip(
    activate: On<Activate>,
    buttons: Query<&AnimClipButton>,
    mut scene: ResMut<SceneState>,
    mut sets: Query<&mut AnimationSet>,
    mut window: ResMut<AnimationState>,
    mut undo: ResMut<Undo>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some((id, entity)) = selection(&scene) else {
        return;
    };
    let Ok(mut set) = sets.get_mut(entity) else {
        return;
    };
    undo.checkpoint(&scene, "Play Animation");
    if set.play(&button.0) {
        window.status = format!("playing {}", button.0);
        store(&mut scene, id, &set);
    } else {
        window.status = set.error.clone().unwrap_or_default();
    }
    window.rev = window.rev.wrapping_add(1);
}

fn stop_clip(
    _: On<Activate>,
    mut scene: ResMut<SceneState>,
    mut sets: Query<&mut AnimationSet>,
    mut window: ResMut<AnimationState>,
    mut undo: ResMut<Undo>,
) {
    let Some((id, entity)) = selection(&scene) else {
        return;
    };
    let Ok(mut set) = sets.get_mut(entity) else {
        return;
    };
    undo.checkpoint(&scene, "Stop Animation");
    set.stop();
    window.status = "stopped".into();
    store(&mut scene, id, &set);
    window.rev = window.rev.wrapping_add(1);
}

fn toggle_loop(
    _: On<Activate>,
    mut scene: ResMut<SceneState>,
    mut sets: Query<&mut AnimationSet>,
    mut window: ResMut<AnimationState>,
    mut undo: ResMut<Undo>,
) {
    let Some((id, entity)) = selection(&scene) else {
        return;
    };
    let Ok(mut set) = sets.get_mut(entity) else {
        return;
    };
    undo.checkpoint(&scene, "Animation Loop");
    set.looping = !set.looping;
    window.status = format!("loop {}", if set.looping { "on" } else { "off" });
    store(&mut scene, id, &set);
    window.rev = window.rev.wrapping_add(1);
}

fn store(scene: &mut SceneState, id: u64, set: &AnimationSet) {
    let animation = AnimationRef {
        clip: set.selected.clone(),
        autoplay: set.playing,
        speed: set.speed,
        looping: set.looping,
    };
    if scene.doc.set_animation(id, Some(animation)) {
        scene.touch();
    }
}

fn tick(
    scene: Res<SceneState>,
    sets: Query<(&AnimationSet, &AnimationClock)>,
    mut fills: Query<&mut Node, With<AnimProgressFill>>,
    mut labels: Query<&mut Text, (With<AnimTimeText>, Without<AnimProgressFill>)>,
) {
    let Some((_, entity)) = selection(&scene) else {
        return;
    };
    let Ok((_, clock)) = sets.get(entity) else {
        return;
    };
    let fraction = if clock.duration > 0.0 {
        (clock.time / clock.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for mut node in &mut fills {
        let want = percent(fraction * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
    let label = format!("{:.2} / {:.2} s", clock.time, clock.duration);
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::{ObjectKind, ObjectTransform};

    fn set() -> AnimationSet {
        AnimationSet {
            clips: vec!["Spin".into(), "Walk".into()],
            selected: Some("Walk".into()),
            playing: true,
            speed: 2.5,
            looping: false,
            ..Default::default()
        }
    }

    #[test]
    fn opening_seeds_drafts_from_the_runtime_set() {
        let mut window = AnimationState::default();
        window.open_with(Some(&set()));
        assert!(window.open);
        assert_eq!(window.speed, "2.50");
        assert_eq!(window.scrub, "0");
    }

    #[test]
    fn persisting_writes_speed_and_loop_to_the_document() {
        let mut state = SceneState::default();
        let id = state.spawn(ObjectKind::Model);
        store(&mut state, id, &set());
        let animation = state.doc.animation(id).expect("animation stored");
        assert_eq!(animation.clip.as_deref(), Some("Walk"));
        assert!(animation.autoplay);
        assert_eq!(animation.speed, 2.5);
        assert!(!animation.looping);
    }

    #[test]
    fn a_model_object_round_trips_through_the_scene_doc() {
        let mut state = SceneState::default();
        let id = state.spawn(ObjectKind::Model);
        store(&mut state, id, &set());
        let back = eme_game_scene::SceneDoc::from_ron(&state.doc.to_ron().unwrap()).unwrap();
        let animation = back.animation(id).unwrap();
        assert_eq!(animation.speed, 2.5);
        assert!(!animation.looping);
        let _ = ObjectTransform::default();
    }
}
