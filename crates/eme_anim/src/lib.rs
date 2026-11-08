//! glTF animation playback shared by the Fabr editor and player.
//!
//! A model root with [`AnimationSource`] gets an [`AnimationSet`] once its glTF
//! asset finishes loading. The set lists every clip in the file; playing a clip
//! drives the [`AnimationPlayer`] the glTF loader created inside the hierarchy.

use eme_animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use eme_animation::{prelude::*, AnimationPlayer};
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
}

/// Clips available on a model and the current playback choice.
#[derive(Component, Clone, Debug, Default)]
pub struct AnimationSet {
    /// Clip names in graph order.
    pub clips: Vec<String>,
    /// Currently selected clip.
    pub selected: Option<String>,
    /// Whether the selected clip is playing.
    pub playing: bool,
    /// Last selection error, such as a missing clip.
    pub error: Option<String>,
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
}

/// Graph asset and node indices built from a model's clips.
#[derive(Component, Clone, Debug)]
pub struct AnimationGraphAsset {
    /// Graph stored in `Assets<AnimationGraph>`.
    pub graph: Handle<AnimationGraph>,
    /// Node for each entry in [`AnimationSet::clips`].
    pub nodes: Vec<AnimationNodeIndex>,
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
            (setup_animations, link_players, drive_animations).chain(),
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
        let (graph, nodes) = AnimationGraph::from_clips(handles);
        let graph = graphs.add(graph);
        let mut set = AnimationSet {
            clips,
            selected: source.selected.clone(),
            playing: source.autoplay,
            error: None,
        };
        if set.selected.is_none() && set.playing {
            set.selected = Some(set.clips[0].clone());
        }
        commands
            .entity(root)
            .insert((set, AnimationGraphAsset { graph, nodes }));
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
    mut sets: Query<
        (&mut AnimationSet, &AnimationGraphAsset, Option<&AnimationPlayerLink>),
        Or<(Changed<AnimationSet>, Added<AnimationPlayerLink>)>,
    >,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (mut set, asset, link) in &mut sets {
        let Some(link) = link else {
            continue;
        };
        let Ok(mut player) = players.get_mut(link.0) else {
            continue;
        };
        player.stop_all();
        if !set.playing {
            continue;
        }
        let Some(name) = set.selected.clone() else {
            continue;
        };
        let Some(index) = set.clips.iter().position(|clip| *clip == name) else {
            set.error = Some(format!("missing clip {name}"));
            set.playing = false;
            continue;
        };
        if let Some(node) = asset.nodes.get(index) {
            player.play(*node).repeat();
            set.error = None;
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
            error: None,
        };
        assert!(!set.play("Fly"));
        assert!(set.error.as_deref().is_some_and(|error| error.contains("missing clip Fly")));
        assert!(!set.playing);
        assert_eq!(set.selected.as_deref(), Some("Spin"));
    }

    #[test]
    fn stopping_clears_the_error_and_playback_flag() {
        let mut set = AnimationSet {
            clips: vec!["Spin".into()],
            selected: Some("Spin".into()),
            playing: true,
            error: Some("old".into()),
        };
        set.stop();
        assert!(!set.playing);
        assert!(set.error.is_none());
        assert_eq!(set.selected.as_deref(), Some("Spin"));
    }
}
