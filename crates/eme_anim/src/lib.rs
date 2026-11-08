//! glTF animation playback shared by the Fabr editor and player.
//!
//! A model root with [`AnimationSource`] gets an [`AnimationSet`] once its glTF
//! asset finishes loading. The set lists every clip in the file; playing a clip
//! drives the [`AnimationPlayer`] the glTF loader created inside the hierarchy.

use eme_animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use eme_animation::{prelude::*, AnimationPlayer, RepeatAnimation};
use eme_app::{App, Plugin, Update};
use eme_asset::{Assets, Handle};
use eme_ecs::prelude::*;
use eme_gltf::Gltf;

/// Model root whose glTF animations should be prepared.
#[derive(Component, Clone, Debug)]
pub struct AnimationSource {
    /// Root glTF asset carrying the clip list.
    pub gltf: Handle<Gltf>,
    /// Clip selected by the scene or inspector.
    pub selected: Option<String>,
    /// Whether the selected clip starts automatically.
    pub autoplay: bool,
    /// Playback speed multiplier.
    pub speed: f32,
    /// Whether the clip repeats forever.
    pub looping: bool,
}

/// Clips available on a model and the current playback choice.
#[derive(Component, Clone, Debug)]
pub struct AnimationSet {
    /// Clip names in graph order.
    pub clips: Vec<String>,
    /// Currently selected clip.
    pub selected: Option<String>,
    /// Whether the selected clip is playing.
    pub playing: bool,
    /// Last selection error, such as a missing clip.
    pub error: Option<String>,
    /// Playback speed multiplier.
    pub speed: f32,
    /// Whether the selected clip loops forever.
    pub looping: bool,
    /// Seek requested by the editor, consumed on the next drive.
    pub seek: Option<f32>,
}

impl Default for AnimationSet {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            selected: None,
            playing: false,
            error: None,
            speed: 1.0,
            looping: true,
            seek: None,
        }
    }
}

impl AnimationSet {
    /// Selects and starts `name` when it exists; returns false otherwise.
    pub fn play(&mut self, name: &str) -> bool {
        if !self.clips.iter().any(|clip| clip == name) {
            self.error = Some(format!("missing clip {name}"));
            self.playing = false;
            return false;
        }
        self.selected = Some(name.to_owned());
        self.playing = true;
        self.error = None;
        true
    }

    /// Stops the current clip.
    pub fn stop(&mut self) {
        self.playing = false;
        self.error = None;
    }

    /// Sets the speed multiplier, clamped to 0..=10.
    pub fn set_speed(&mut self, speed: f32) {
        if speed.is_finite() {
            self.speed = speed.clamp(0.0, 10.0);
        }
    }

    /// Requests a seek, clamped to the clip's duration when known.
    pub fn seek_to(&mut self, time: f32, duration: f32) {
        if time.is_finite() {
            self.seek = Some(time.clamp(0.0, duration.max(0.0)));
        }
    }

    /// Plays the selected clip from the start.
    pub fn restart(&mut self) {
        self.seek = Some(0.0);
        self.playing = true;
    }
}

/// Playback progress of the linked animation player.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AnimationClock {
    /// Current playback time in seconds.
    pub time: f32,
    /// Duration of the active clip in seconds.
    pub duration: f32,
}

/// What [`drive_animations`] last applied, so time-only changes do not replay.
#[derive(Component, Clone, Debug, Default)]
pub struct AppliedAnimation {
    /// Clip last started.
    pub selected: Option<String>,
    /// Whether playback was on.
    pub playing: bool,
    /// Speed last applied.
    pub speed: f32,
    /// Loop flag last applied.
    pub looping: bool,
}

/// Graph asset and node indices built from a model's clips.
#[derive(Component, Clone, Debug)]
pub struct AnimationGraphAsset {
    /// Graph stored in `Assets<AnimationGraph>`.
    pub graph: Handle<AnimationGraph>,
    /// Node for each entry in [`AnimationSet::clips`].
    pub nodes: Vec<AnimationNodeIndex>,
    /// Clip asset behind each entry, for durations.
    pub clips: Vec<Handle<AnimationClip>>,
}

/// Entity carrying the glTF loader's [`AnimationPlayer`].
#[derive(Component, Clone, Copy, Debug)]
pub struct AnimationPlayerLink(pub Entity);

/// Registers the animation setup and playback systems.
pub struct AnimationRuntimePlugin;

impl Plugin for AnimationRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                setup_animations,
                link_players,
                drive_animations,
                sync_animation_clock,
            )
                .chain(),
        );
    }
}

/// Builds clip lists and graphs once the glTF asset is available.
pub fn setup_animations(
    mut commands: Commands,
    sources: Query<(Entity, &AnimationSource), Without<AnimationSet>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for (root, source) in &sources {
        let Some(gltf) = gltfs.get(&source.gltf) else {
            continue;
        };
        let mut clips = Vec::new();
        let mut handles = Vec::new();
        let mut named: Vec<(String, Handle<AnimationClip>)> = gltf
            .named_animations
            .iter()
            .map(|(name, handle)| (name.to_string(), handle.clone()))
            .collect();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, handle) in named {
            clips.push(name);
            handles.push(handle);
        }
        for (index, handle) in gltf.animations.iter().enumerate() {
            if !handles.iter().any(|known| known.id() == handle.id()) {
                clips.push(format!("Animation {index}"));
                handles.push(handle.clone());
            }
        }
        if clips.is_empty() {
            commands.entity(root).insert(AnimationSet {
                error: Some("model has no animation clips".into()),
                ..Default::default()
            });
            continue;
        }
        let (graph, nodes) = AnimationGraph::from_clips(handles.clone());
        let graph = graphs.add(graph);
        let mut set = AnimationSet {
            clips,
            selected: source.selected.clone(),
            playing: source.autoplay,
            error: None,
            speed: source.speed,
            looping: source.looping,
            seek: None,
        };
        if set.selected.is_none() && set.playing {
            set.selected = Some(set.clips[0].clone());
        }
        commands.entity(root).insert((
            set,
            AnimationGraphAsset {
                graph,
                nodes,
                clips: handles,
            },
            AnimationClock::default(),
        ));
    }
}

/// Attaches the graph to the loader's player once the hierarchy exists.
pub fn link_players(
    mut commands: Commands,
    roots: Query<(Entity, &AnimationGraphAsset), Without<AnimationPlayerLink>>,
    players: Query<(Entity, &ChildOf), With<AnimationPlayer>>,
    parents: Query<&ChildOf>,
) {
    for (root, asset) in &roots {
        let Some(player) = players
            .iter()
            .find(|(entity, _)| is_descendant(*entity, root, &parents))
            .map(|(entity, _)| entity)
        else {
            continue;
        };
        commands
            .entity(player)
            .insert(AnimationGraphHandle(asset.graph.clone()));
        commands.entity(root).insert(AnimationPlayerLink(player));
    }
}

/// Applies playback changes to the linked player.
pub fn drive_animations(
    mut commands: Commands,
    mut sets: Query<(
        Entity,
        &mut AnimationSet,
        &AnimationGraphAsset,
        Option<&AnimationPlayerLink>,
        Option<&AppliedAnimation>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (entity, mut set, asset, link, applied) in &mut sets {
        let Some(link) = link else {
            continue;
        };
        let seek = set.seek.take();
        let changed = applied.is_none_or(|applied| {
            applied.selected != set.selected
                || applied.playing != set.playing
                || applied.speed != set.speed
                || applied.looping != set.looping
                || seek.is_some()
        });
        if !changed {
            continue;
        }
        let Ok(mut player) = players.get_mut(link.0) else {
            continue;
        };
        player.stop_all();
        let node = set
            .selected
            .clone()
            .and_then(|name| {
                let index = set.clips.iter().position(|clip| *clip == name)?;
                asset.nodes.get(index).copied()
            });
        match (set.playing, node) {
            (true, Some(node)) => {
                let active = player.play(node);
                active.set_repeat(if set.looping {
                    RepeatAnimation::Forever
                } else {
                    RepeatAnimation::Never
                });
                active.set_speed(set.speed.max(0.0));
                if let Some(time) = seek {
                    active.seek_to(time.max(0.0));
                }
                set.error = None;
            }
            (true, None) => {
                set.error = Some("select a clip to play".into());
                set.playing = false;
            }
            (false, Some(node)) => {
                // a scrub while stopped shows the pose without advancing time
                if let Some(time) = seek {
                    let active = player.play(node);
                    active.set_speed(0.0).seek_to(time.max(0.0));
                }
            }
            (false, None) => {}
        }
        let applied = AppliedAnimation {
            selected: set.selected.clone(),
            playing: set.playing,
            speed: set.speed,
            looping: set.looping,
        };
        commands.entity(entity).insert(applied);
    }
}

/// Copies the active player's progress into [`AnimationClock`] for the editor.
pub fn sync_animation_clock(
    mut clocks: Query<(
        &AnimationSet,
        &AnimationGraphAsset,
        &AnimationPlayerLink,
        &mut AnimationClock,
    )>,
    players: Query<&AnimationPlayer>,
    clips: Res<Assets<AnimationClip>>,
) {
    for (set, asset, link, mut clock) in &mut clocks {
        let Some(index) = set
            .selected
            .as_ref()
            .and_then(|name| set.clips.iter().position(|clip| clip == name))
        else {
            continue;
        };
        let Some(node) = asset.nodes.get(index) else {
            continue;
        };
        let Ok(player) = players.get(link.0) else {
            continue;
        };
        let Some(active) = player.animation(*node) else {
            continue;
        };
        let time = active.seek_time();
        let duration = asset
            .clips
            .get(index)
            .and_then(|clip| clips.get(clip))
            .map_or(0.0, AnimationClip::duration);
        if (clock.time - time).abs() > 1e-5 || (clock.duration - duration).abs() > 1e-5 {
            clock.time = time;
            clock.duration = duration;
        }
    }
}

fn is_descendant(entity: Entity, root: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut current = entity;
    while let Ok(parent) = parents.get(current) {
        let parent = parent.parent();
        if parent == root {
            return true;
        }
        if parent == current {
            break;
        }
        current = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_known_clip_starts_it() {
        let mut set = AnimationSet {
            clips: vec!["Spin".into(), "Walk".into()],
            ..Default::default()
        };
        assert!(set.play("Walk"));
        assert_eq!(set.selected.as_deref(), Some("Walk"));
        assert!(set.playing);
        assert!(set.error.is_none());
    }

    #[test]
    fn a_missing_clip_is_reported_and_does_not_start() {
        let mut set = AnimationSet {
            clips: vec!["Spin".into()],
            selected: Some("Spin".into()),
            playing: true,
            ..Default::default()
        };
        assert!(!set.play("Fly"));
        assert!(set.error.as_deref().is_some_and(|error| error.contains("missing clip Fly")));
        assert!(!set.playing);
        assert_eq!(set.selected.as_deref(), Some("Spin"));
    }

    #[test]
    fn speed_and_seek_are_clamped() {
        let mut set = AnimationSet::default();
        set.set_speed(40.0);
        assert_eq!(set.speed, 10.0);
        set.set_speed(f32::NAN);
        assert_eq!(set.speed, 10.0);
        set.seek_to(9.0, 2.0);
        assert_eq!(set.seek, Some(2.0));
        set.seek_to(-1.0, 2.0);
        assert_eq!(set.seek, Some(0.0));
    }

    #[test]
    fn looping_defaults_on_at_normal_speed() {
        let set = AnimationSet::default();
        assert!(set.looping);
        assert_eq!(set.speed, 1.0);
        assert!(set.seek.is_none());
    }

    #[test]
    fn stopping_clears_the_error_and_playback_flag() {
        let mut set = AnimationSet {
            clips: vec!["Spin".into()],
            selected: Some("Spin".into()),
            playing: true,
            error: Some("old".into()),
            ..Default::default()
        };
        set.stop();
        assert!(!set.playing);
        assert!(set.error.is_none());
        assert_eq!(set.selected.as_deref(), Some("Spin"));
    }
}
