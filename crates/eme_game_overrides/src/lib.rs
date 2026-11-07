//! Model-node overrides shared by the editor and the player.
//!
//! The source asset stays untouched: author edits are stored on the scene object
//! keyed by [`NodeKey`], resolved against the spawned node tree on every spawn and
//! every hot reload, and applied through one system in both binaries.

use std::collections::HashMap;

use eme_camera::visibility::Visibility;
use eme_ecs::prelude::*;
use eme_game_scene::{NodeKey, NodeOverride, OverrideOp};
use eme_math::prelude::*;
use eme_mesh::Mesh3d;
use eme_transform::prelude::Transform;

/// Author edits for one instanced model, placed on the model root entity.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct ModelOverrides(pub Vec<NodeOverride>);

/// Identity of one spawned glTF node.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct ModelNode {
    /// Key built from the node's ordinal path and name.
    pub key: NodeKey,
    /// Nesting depth, scene roots at 0.
    pub depth: usize,
}

/// One node of the current instance, including nodes excluded from spawning.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelNodeInfo {
    /// Shared node identity.
    pub key: NodeKey,
    /// Nesting depth, scene roots at 0.
    pub depth: usize,
    /// False when the node is excluded and no live entity exists.
    pub present: bool,
    /// Live entity, when spawned.
    pub entity: Option<Entity>,
}

/// Node tree built from the last spawn or reload, for the tree and inspector.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct ModelNodeCatalog {
    /// Nodes in depth-first order, including excluded ones.
    pub nodes: Vec<ModelNodeInfo>,
}

/// Why an override no longer applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrphanReason {
    /// No node has the key's name.
    Missing,
    /// A node still sits at the key's path under a different name.
    Renamed {
        /// Name found at the recorded path.
        found: String,
    },
}

/// An override that could not be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanOverride {
    /// Key the author edited.
    pub key: NodeKey,
    /// Why it did not match.
    pub reason: OrphanReason,
}

/// Result of the last application, for warnings and the inspector.
#[derive(Component, Clone, Default, PartialEq, Debug)]
pub struct ModelOverrideStatus {
    /// Overrides that matched nothing.
    pub orphans: Vec<OrphanOverride>,
    /// Overrides applied through the unique-name move rule.
    pub moved: Vec<(NodeKey, NodeKey)>,
    /// Number of overrides applied to live nodes.
    pub applied: usize,
    /// Bumped every time this status is rebuilt.
    pub revision: u64,
}

/// Where an override's key landed in the current tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchTarget {
    /// The exact key exists.
    Exact(NodeKey),
    /// No exact key, but exactly one node carries the same name.
    Moved {
        /// Recorded key.
        from: NodeKey,
        /// Node it was applied to.
        to: NodeKey,
    },
    /// Nothing was applied; the key is orphaned.
    Orphaned(OrphanReason),
}

/// Resolves one key against the current node keys.
///
/// Exact match wins. Otherwise a name that is unique among the nodes is treated
/// as a move. If the recorded path still exists under another name the override is
/// orphaned and reports the new name; anything else is missing.
pub fn match_key(key: &NodeKey, nodes: &[NodeKey]) -> MatchTarget {
    if nodes.iter().any(|node| node == key) {
        return MatchTarget::Exact(key.clone());
    }
    let mut named = nodes.iter().filter(|node| node.name == key.name);
    if let Some(only) = named.next()
        && named.next().is_none()
    {
        return MatchTarget::Moved {
            from: key.clone(),
            to: only.clone(),
        };
    }
    if let Some(found) = nodes.iter().find(|node| node.path == key.path) {
        return MatchTarget::Orphaned(OrphanReason::Renamed {
            found: found.name.clone(),
        });
    }
    MatchTarget::Orphaned(OrphanReason::Missing)
}

fn walk_nodes(
    entity: Entity,
    parent: &NodeKey,
    depth: usize,
    children: &Query<&Children>,
    names: &Query<&Name>,
    meshes: &Query<(), With<Mesh3d>>,
    out: &mut Vec<(Entity, NodeKey, usize)>,
) {
    let Ok(kids) = children.get(entity) else {
        return;
    };
    let mut ordinal = 0;
    for child in kids.iter() {
        // glTF primitives are spawned as mesh children of a node; they are not
        // nodes in the imported tree and must not consume an ordinal
        if meshes.contains(child) {
            continue;
        }
        let name = names
            .get(child)
            .map_or_else(|_| String::new(), |name| name.as_str().to_owned());
        let key = parent.child(ordinal, &name);
        ordinal += 1;
        out.push((child, key.clone(), depth));
        walk_nodes(child, &key, depth + 1, children, names, meshes, out);
    }
}

/// Tags a model's nodes, applies its overrides, and records the result.
///
/// Runs for the editor and the player; exclusion despawns the node subtree, so an
/// excluded node has no entity until a reload reproduces the tree.
pub fn sync_model_nodes(
    mut commands: Commands,
    mut roots: Query<
        (
            Entity,
            &ModelOverrides,
            Option<&Children>,
            Option<&mut ModelNodeCatalog>,
            Option<&mut ModelOverrideStatus>,
        ),
        Or<(Changed<ModelOverrides>, Changed<Children>)>,
    >,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
) {
    for (root, overrides, kids, catalog, status) in &mut roots {
        let previous = catalog
            .as_ref()
            .map_or_else(Vec::new, |catalog| catalog.nodes.clone());
        let mut found = Vec::new();
        // the world asset is one scene container; imported node ordinals start
        // at the glTF scene roots below it
        if let Some(kids) = kids {
            for scene in kids.iter() {
                walk_nodes(
                    scene,
                    &NodeKey::default(),
                    0,
                    &children,
                    &names,
                    &meshes,
                    &mut found,
                );
            }
        }

        let mut known: Vec<NodeKey> = found.iter().map(|(_, key, _)| key.clone()).collect();
        for info in &previous {
            if !known.contains(&info.key) {
                known.push(info.key.clone());
            }
        }

        let mut live: HashMap<NodeKey, Entity> = HashMap::new();
        for (entity, key, depth) in &found {
            live.insert(key.clone(), *entity);
            commands.entity(*entity).insert(ModelNode {
                key: key.clone(),
                depth: *depth,
            });
        }

        let revision = status.as_ref().map_or(1, |status| status.revision + 1);
        let mut next = ModelOverrideStatus {
            revision,
            ..Default::default()
        };
        let mut excluded: Vec<NodeKey> = Vec::new();
        let mut hidden: Vec<NodeKey> = Vec::new();

        for entry in &overrides.0 {
            let target = match_key(&entry.key, &known);
            let (key, applied) = match &target {
                MatchTarget::Exact(key) => (Some(key.clone()), true),
                MatchTarget::Moved { from, to } => {
                    next.moved.push((from.clone(), to.clone()));
                    (Some(to.clone()), true)
                }
                MatchTarget::Orphaned(reason) => {
                    next.orphans.push(OrphanOverride {
                        key: entry.key.clone(),
                        reason: reason.clone(),
                    });
                    (None, false)
                }
            };
            if applied {
                next.applied += 1;
            }
            let Some(key) = key else {
                continue;
            };
            let Some(entity) = live.get(&key).copied() else {
                continue;
            };
            match &entry.op {
                OverrideOp::SetTransform(transform) => {
                    commands.entity(entity).insert(Transform {
                        translation: Vec3::from_array(transform.translation),
                        rotation: Quat::from_array(transform.rotation),
                        scale: Vec3::from_array(transform.scale),
                    });
                }
                OverrideOp::SetVisible { visible } => {
                    let visibility = if *visible {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    commands.entity(entity).insert(visibility);
                    hidden.push(key);
                }
                OverrideOp::SetExcluded { excluded: on } => {
                    if *on {
                        excluded.push(key);
                        commands.entity(entity).try_despawn();
                    }
                }
            }
        }

        let mut nodes: Vec<ModelNodeInfo> = found
            .iter()
            .map(|(entity, key, depth)| ModelNodeInfo {
                key: key.clone(),
                depth: *depth,
                present: true,
                entity: Some(*entity),
            })
            .collect();
        for info in &previous {
            if !nodes.iter().any(|node| node.key == info.key) {
                nodes.push(ModelNodeInfo {
                    present: false,
                    entity: None,
                    ..info.clone()
                });
            }
        }
        for node in &mut nodes {
            if excluded.iter().any(|key| node.key.is_within(key)) {
                node.present = false;
                node.entity = None;
            }
            if hidden.contains(&node.key) {
                node.present = true;
            }
        }

        if let Some(mut catalog) = catalog {
            *catalog = ModelNodeCatalog { nodes };
        } else {
            commands.entity(root).insert(ModelNodeCatalog { nodes });
        }
        if let Some(mut status) = status {
            *status = next;
        } else {
            commands.entity(root).insert(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_ecs::system::RunSystemOnce;

    fn key(path: &[u32], name: &str) -> NodeKey {
        NodeKey {
            path: path.to_vec(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn exact_matches_first() {
        let nodes = vec![key(&[0], "A"), key(&[0, 1], "B")];
        assert_eq!(
            match_key(&key(&[0, 1], "B"), &nodes),
            MatchTarget::Exact(key(&[0, 1], "B"))
        );
    }

    #[test]
    fn a_unique_name_at_a_new_path_is_a_move() {
        let nodes = vec![key(&[2], "A"), key(&[3], "B")];
        assert_eq!(
            match_key(&key(&[0], "B"), &nodes),
            MatchTarget::Moved {
                from: key(&[0], "B"),
                to: key(&[3], "B")
            }
        );
    }

    #[test]
    fn a_duplicate_name_is_not_guessed() {
        let nodes = vec![key(&[1], "Leg"), key(&[2], "Leg")];
        assert_eq!(
            match_key(&key(&[0], "Leg"), &nodes),
            MatchTarget::Orphaned(OrphanReason::Missing)
        );
    }

    #[test]
    fn a_renamed_node_at_the_same_path_is_reported() {
        let nodes = vec![key(&[0], "NewName")];
        assert_eq!(
            match_key(&key(&[0], "OldName"), &nodes),
            MatchTarget::Orphaned(OrphanReason::Renamed {
                found: "NewName".into()
            })
        );
    }

    fn world_with_tree() -> (World, Entity, Entity, Entity) {
        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![
                NodeOverride {
                    key: key(&[0], "Seat"),
                    op: OverrideOp::SetTransform(
                        eme_game_scene::ObjectTransform::from_translation([1.0, 2.0, 3.0]),
                    ),
                },
                NodeOverride {
                    key: key(&[0, 0], "Leg"),
                    op: OverrideOp::SetVisible { visible: false },
                },
            ]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let seat = world.spawn(Name::new("Seat")).id();
        let leg = world.spawn(Name::new("Leg")).id();
        world.entity_mut(seat).add_children(&[leg]);
        world.entity_mut(scene).add_children(&[seat]);
        world.entity_mut(root).add_children(&[scene]);
        (world, root, seat, leg)
    }

    #[test]
    fn the_system_applies_transform_visibility_and_reports_status() {
        let (mut world, root, seat, leg) = world_with_tree();
        world.run_system_once(sync_model_nodes).unwrap();

        let transform = *world.get::<Transform>(seat).unwrap();
        assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(*world.get::<Visibility>(leg).unwrap(), Visibility::Hidden);
        let catalog = world.get::<ModelNodeCatalog>(root).unwrap();
        assert_eq!(catalog.nodes.len(), 2);
        assert_eq!(catalog.nodes[0].key, key(&[0], "Seat"));
        assert_eq!(catalog.nodes[1].depth, 1);
        let status = world.get::<ModelOverrideStatus>(root).unwrap();
        assert_eq!(status.applied, 2);
        assert!(status.orphans.is_empty());
    }

    #[test]
    fn excluded_nodes_are_despawned_and_kept_in_the_catalog() {
        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![NodeOverride {
                key: key(&[0], "Seat"),
                op: OverrideOp::SetExcluded { excluded: true },
            }]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let seat = world.spawn(Name::new("Seat")).id();
        world.entity_mut(scene).add_children(&[seat]);
        world.entity_mut(root).add_children(&[scene]);
        world.run_system_once(sync_model_nodes).unwrap();

        assert!(world.get_entity(seat).is_err());
        let catalog = world.get::<ModelNodeCatalog>(root).unwrap();
        assert_eq!(catalog.nodes.len(), 1);
        assert!(!catalog.nodes[0].present);
    }

    #[test]
    fn a_moved_override_applies_at_the_unique_name() {
        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![NodeOverride {
                key: key(&[0], "Seat"),
                op: OverrideOp::SetTransform(eme_game_scene::ObjectTransform::from_translation([
                    3.0, 0.0, 0.0,
                ])),
            }]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let other = world.spawn(Name::new("Other")).id();
        let seat = world.spawn(Name::new("Seat")).id();
        world.entity_mut(scene).add_children(&[other, seat]);
        world.entity_mut(root).add_children(&[scene]);
        world.run_system_once(sync_model_nodes).unwrap();

        assert_eq!(
            world.get::<Transform>(seat).unwrap().translation,
            Vec3::new(3.0, 0.0, 0.0)
        );
        let status = world.get::<ModelOverrideStatus>(root).unwrap();
        assert!(status.orphans.is_empty());
        assert_eq!(status.moved, vec![(key(&[0], "Seat"), key(&[1], "Seat"))]);
    }

    #[test]
    fn an_override_is_reapplied_after_the_tree_is_replaced() {
        let (mut world, root, seat, _leg) = world_with_tree();
        world.run_system_once(sync_model_nodes).unwrap();
        assert_eq!(
            world.get::<Transform>(seat).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0)
        );

        let scene = world.get::<Children>(root).unwrap()[0];
        world.entity_mut(seat).despawn();
        let replacement = world.spawn(Name::new("Seat")).id();
        world.entity_mut(scene).add_children(&[replacement]);
        world.run_system_once(sync_model_nodes).unwrap();

        assert_eq!(
            world.get::<Transform>(replacement).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0)
        );
        let catalog = world.get::<ModelNodeCatalog>(root).unwrap();
        assert_eq!(catalog.nodes[0].key, key(&[0], "Seat"));
    }

    #[test]
    fn an_orphan_stays_reported_after_another_pass() {
        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![NodeOverride {
                key: key(&[0], "Old"),
                op: OverrideOp::SetVisible { visible: false },
            }]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let seat = world.spawn(Name::new("New")).id();
        world.entity_mut(scene).add_children(&[seat]);
        world.entity_mut(root).add_children(&[scene]);
        world.run_system_once(sync_model_nodes).unwrap();

        let set = world.get::<ModelOverrides>(root).unwrap().clone();
        *world.get_mut::<ModelOverrides>(root).unwrap() = set;
        world.run_system_once(sync_model_nodes).unwrap();

        let status = world.get::<ModelOverrideStatus>(root).unwrap();
        assert_eq!(status.orphans.len(), 1);
        assert_eq!(
            world.get::<ModelOverrides>(root).unwrap().0.len(),
            1,
            "the orphan must never be dropped"
        );
    }

    #[test]
    fn an_orphan_is_reported_and_never_applied() {
        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![NodeOverride {
                key: key(&[0], "Old"),
                op: OverrideOp::SetVisible { visible: false },
            }]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let seat = world.spawn(Name::new("New")).id();
        world.entity_mut(scene).add_children(&[seat]);
        world.entity_mut(root).add_children(&[scene]);
        world.run_system_once(sync_model_nodes).unwrap();

        let status = world.get::<ModelOverrideStatus>(root).unwrap();
        assert_eq!(status.applied, 0);
        assert_eq!(
            status.orphans[0].reason,
            OrphanReason::Renamed {
                found: "New".into()
            }
        );
        assert!(world.get::<Visibility>(seat).is_none());
    }
}
