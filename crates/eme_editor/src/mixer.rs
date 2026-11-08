//! Audio mixer: master, SFX and music bus levels applied to the running app.

use std::path::PathBuf;

use eme::audio::{GlobalVolume, Volume};
use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button};
use eme_editor_project::{AudioSettings, ProjectSettings};

use crate::console::ConsoleLog;
use crate::screens::editor::chrome::{self, spacer, Part, FONT_SIZE};
use crate::tokens;

/// Number of clickable steps in a bus strip; step 20 is 1.0.
const STEPS: u32 = 20;

/// One mixer bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MixBus {
    /// Master gain, applied through `GlobalVolume`.
    #[default]
    Master,
    /// Sound-effect gain.
    Sfx,
    /// Music gain.
    Music,
}

impl MixBus {
    /// Every bus, in display order.
    pub const ALL: [MixBus; 3] = [MixBus::Master, MixBus::Sfx, MixBus::Music];

    /// Label shown in the mixer.
    pub fn label(self) -> &'static str {
        match self {
            MixBus::Master => "Master",
            MixBus::Sfx => "SFX",
            MixBus::Music => "Music",
        }
    }

    /// Current linear gain from `audio`.
    pub fn get(self, audio: &AudioSettings) -> f32 {
        match self {
            MixBus::Master => audio.master_volume,
            MixBus::Sfx => audio.sfx_volume,
            MixBus::Music => audio.music_volume,
        }
    }

    fn set(self, audio: &mut AudioSettings, value: f32) {
        match self {
            MixBus::Master => audio.master_volume = value,
            MixBus::Sfx => audio.sfx_volume = value,
            MixBus::Music => audio.music_volume = value,
        }
    }
}

/// Mixer window state and the draft bus levels.
#[derive(Resource)]
pub struct MixerState {
    /// Whether the window is visible.
    pub open: bool,
    /// Draft levels, seeded from the project file on open.
    pub audio: AudioSettings,
    /// Save or validation message.
    pub status: String,
    /// Bumped to rebuild the strips.
    pub rev: u64,
    root: PathBuf,
}

impl MixerState {
    /// Builds state from the current project file.
    pub fn load() -> Self {
        let root = crate::build::workspace_dir();
        let path = root.join(eme_editor_project::SETTINGS_FILE);
        let audio = ProjectSettings::load_or_default(&path)
            .map(|settings| settings.audio)
            .unwrap_or_default();
        Self {
            open: false,
            audio,
            status: String::new(),
            rev: 0,
            root,
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(eme_editor_project::SETTINGS_FILE)
    }

    /// Sets one bus from a strip step, clamped to 0..=1.
    pub fn set_bus(&mut self, bus: MixBus, level: u32) -> bool {
        let value = level_volume(level);
        if (bus.get(&self.audio) - value).abs() < f32::EPSILON {
            return false;
        }
        bus.set(&mut self.audio, value);
        self.rev = self.rev.wrapping_add(1);
        true
    }
}

/// Linear gain for a strip step.
pub fn level_volume(level: u32) -> f32 {
    level.min(STEPS) as f32 / STEPS as f32
}

/// Installs the mixer resource and window.
pub fn install(app: &mut App) {
    app.insert_resource(MixerState::load())
        .add_systems(Update, sync);
}

/// Root of the floating mixer window.
#[derive(Component, Clone, Copy, Default)]
pub struct MixerRoot;

/// Host for the rebuilt strips.
#[derive(Component, Clone, Copy, Default)]
pub struct MixerRows;

/// One clickable level segment.
#[derive(Component, Clone, Copy, Default)]
pub struct MixSegment(pub MixBus, pub u32);

/// The mixer overlay.
pub fn panel() -> Part {
    let host = Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(4),
            padding: px(4),
        }
        MixerRows
    }) as Part;
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(560),
            top: px(120),
            width: px(470),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(5),
            overflow: Overflow::clip(),
        }
        MixerRoot
        GlobalZIndex(335)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [ {host} ]
    })
}

/// Shows or hides the window and rebuilds the strips.
pub fn sync(
    state: Res<MixerState>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<MixerRoot>>,
    hosts: Query<Entity, With<MixerRows>>,
    mut shown: Local<Option<(Entity, u64, bool)>>,
) {
    for mut root in &mut roots {
        let want = if state.open {
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
    let signature = (host, state.rev, state.open);
    if shown.as_ref() == Some(&signature) {
        return;
    }
    *shown = Some(signature);
    commands.entity(host).despawn_children();
    if !state.open {
        return;
    }

    let title = text("Audio Mixer", FONT_SIZE, tokens::TEXT);
    commands.spawn_scene(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            height: px(28),
            padding: UiRect::horizontal(px(8)),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        ChildOf(host)
        Children [ {title} -- {spacer()} ]
    });

    for bus in MixBus::ALL {
        bus_strip(&mut commands, host, &state, bus);
    }

    if !state.status.is_empty() {
        let status = text(state.status.clone(), FONT_SIZE, tokens::TEXT_DIM);
        commands.spawn_scene(bsn! {
            Node { padding: UiRect::horizontal(px(8)) }
            ChildOf(host)
            Children [ {status} ]
        });
    }

    let save = text("Save", FONT_SIZE, tokens::TEXT);
    let close = text("Close", FONT_SIZE, tokens::TEXT);
    let actions = commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(4),
                padding: UiRect::all(px(6)),
                height: px(34),
            }
            ChildOf(host)
        })
        .id();
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            SaveMixer
            ChildOf(actions)
            Children [ {save} ]
        })
        .observe(save_mixer);
    commands
        .spawn_scene(bsn! {
            Node { height: px(24), padding: UiRect::horizontal(px(10)), display: Display::Flex, align_items: AlignItems::Center }
            Button
            CloseMixer
            ChildOf(actions)
            Children [ {close} ]
        })
        .observe(close_mixer);
}

/// Save button marker.
#[derive(Component, Clone, Copy, Default)]
pub struct SaveMixer;

/// Close button marker.
#[derive(Component, Clone, Copy, Default)]
pub struct CloseMixer;

fn bus_strip(commands: &mut Commands, host: Entity, state: &MixerState, bus: MixBus) {
    let label = text(bus.label(), FONT_SIZE, tokens::TEXT);
    let value = text(
        format!("{:.2}", bus.get(&state.audio)),
        FONT_SIZE,
        tokens::ACCENT,
    );
    let row = commands
        .spawn_scene(bsn! {
            Node {
                height: px(26),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(8)),
                column_gap: px(3),
            }
            ChildOf(host)
            Children [ {label} -- {spacer()} ]
        })
        .id();
    let current = bus.get(&state.audio);
    for level in 0..=STEPS {
        let color = if level_volume(level) <= current + 0.0001 {
            tokens::ACCENT
        } else {
            tokens::FIELD
        };
        commands
            .spawn_scene(bsn! {
                Node {
                    width: px(12),
                    height: px(18),
                    border_radius: px(2),
                }
                Button
                MixSegment(bus, level)
                ThemeBackgroundColor(color)
                ChildOf(row)
            })
            .observe(set_level);
    }
    commands.spawn_scene(bsn! {
        Node {
            height: px(18),
            display: Display::Flex,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
        }
        ChildOf(host)
        Children [ {value} ]
    });
}

fn set_level(
    activate: On<Activate>,
    segments: Query<&MixSegment>,
    mut state: ResMut<MixerState>,
    mut mix: ResMut<eme_script::AudioMix>,
    mut volume: ResMut<GlobalVolume>,
) {
    let Ok(segment) = segments.get(activate.entity) else {
        return;
    };
    if state.set_bus(segment.0, segment.1) {
        apply(&state.audio, &mut mix, &mut volume);
    }
}

fn apply(audio: &AudioSettings, mix: &mut eme_script::AudioMix, volume: &mut GlobalVolume) {
    *mix = eme_script::AudioMix {
        sfx: audio.sfx_volume,
        music: audio.music_volume,
    };
    *volume = GlobalVolume::new(Volume::Linear(audio.master_volume));
}

fn save_mixer(
    _: On<Activate>,
    mut state: ResMut<MixerState>,
    mut log: ResMut<ConsoleLog>,
) {
    let path = state.path();
    let mut settings = match ProjectSettings::load_or_default(&path) {
        Ok(settings) => settings,
        Err(err) => {
            state.status = err;
            state.rev = state.rev.wrapping_add(1);
            return;
        }
    };
    settings.audio = state.audio.clone();
    match settings.save(&path) {
        Ok(()) => {
            state.status = format!("saved {}", path.display());
            log.push(format!("saved audio mixer to {}", path.display()));
        }
        Err(err) => state.status = err,
    }
    state.rev = state.rev.wrapping_add(1);
}

fn close_mixer(_: On<Activate>, mut state: ResMut<MixerState>) {
    state.open = false;
    state.rev = state.rev.wrapping_add(1);
}

/// Opens the mixer, refreshing its draft from the project file.
pub fn open(state: &mut MixerState) {
    let path = state.path();
    if let Ok(settings) = ProjectSettings::load_or_default(&path) {
        state.audio = settings.audio;
    }
    state.open = true;
    state.status.clear();
    state.rev = state.rev.wrapping_add(1);
}

fn text(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    chrome::text(value, size, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_map_to_twentieths() {
        assert_eq!(level_volume(0), 0.0);
        assert_eq!(level_volume(10), 0.5);
        assert_eq!(level_volume(20), 1.0);
        assert_eq!(level_volume(99), 1.0);
    }

    #[test]
    fn setting_a_bus_updates_only_that_channel() {
        let mut state = MixerState {
            open: false,
            audio: AudioSettings::default(),
            status: String::new(),
            rev: 0,
            root: std::env::temp_dir(),
        };
        assert!(state.set_bus(MixBus::Sfx, 10));
        assert_eq!(state.audio.sfx_volume, 0.5);
        assert_eq!(state.audio.master_volume, 1.0);
        assert_eq!(state.audio.music_volume, 1.0);
        assert_eq!(state.rev, 1);
        assert!(!state.set_bus(MixBus::Sfx, 10));
        assert!(state.set_bus(MixBus::Master, 5));
        assert_eq!(state.audio.master_volume, 0.25);
    }
}
