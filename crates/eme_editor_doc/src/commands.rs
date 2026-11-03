use core::any::Any;

use eme_ecs::{
    entity::{Entity, EntityHashMap, EntityHashSet},
    hierarchy::{ChildOf, Children},
    name::Name,
    reflect::AppTypeRegistry,
    world::World,
};
use eme_editor_core::history::Op;
use eme_math::Vec3;
use eme_transform::prelude::Transform;
use eme_world_serialization::{DynamicWorld, DynamicWorldBuilder};

use crate::DocState;

/// Spawns an entity, optionally parented. Undo despawns it again.
pub struct SpawnEntity {
    /// `Name` given to the new entity.
    pub name: String,
    /// Parent to attach to.
    pub parent: Option<Entity>,
    /// Entity produced by the last [`Op::apply`].
    pub entity: Option<Entity>,
}

impl SpawnEntity {
    /// A spawn of `name` under `parent`.
    pub fn new(name: impl Into<String>, parent: Option<Entity>) -> Self {
        Self {
            name: name.into(),
            parent,
            entity: None,
        }
    }
}

impl Op<DocState> for SpawnEntity {
    fn name(&self) -> &str {
        "spawn entity"
    }

    fn apply(&mut self, state: &mut DocState) {
        let entity = state.world_mut().spawn(Name::new(self.name.clone())).id();
        if let Some(parent) = self.parent
            && state.world_mut().get_entity(parent).is_ok()
        {
            state.world_mut().entity_mut(entity).insert(ChildOf(parent));
        }
        self.entity = Some(entity);
        state.touch();
    }

    fn revert(&mut self, state: &mut DocState) {
        if let Some(entity) = self.entity {
            state.world_mut().try_despawn(entity).ok();
        }
        state.touch();
    }
}

/// Despawns an entity and everything under it. Undo re-spawns the captured snapshot.
pub struct DespawnEntity {
    /// Entity to despawn; after [`Op::apply`] this is the bumped id of the same index.
    pub entity: Entity,
    /// Copy of the entity and its descendants, taken before the despawn.
    pub snapshot: DynamicWorld,
}

impl DespawnEntity {
    /// Captures `entity` and its descendants, or `None` when `entity` is not spawned.
    pub fn capture(world: &World, entity: Entity) -> Option<Self> {
        if world.get_entity(entity).is_err() {
            return None;
        }
        let registry = world.resource::<AppTypeRegistry>().read();
        let snapshot = DynamicWorldBuilder::from_world(world, &registry)
            .extract_entities(subtree(world, entity).into_iter())
            .build();
        Some(Self { entity, snapshot })
    }
}

impl Op<DocState> for DespawnEntity {
    fn name(&self) -> &str {
        "despawn entity"
    }

    fn apply(&mut self, state: &mut DocState) {
        if let Some(entity) = state.world_mut().despawn_no_free(self.entity) {
            self.entity = entity;
        }
        state.touch();
    }

    fn revert(&mut self, state: &mut DocState) {
        let world = state.world_mut();
        if world.spawn_empty_at(self.entity).is_err() {
            return;
        }
        let root = self
            .snapshot
            .entities
            .iter()
            .find(|dynamic| {
                !dynamic
                    .components
                    .iter()
                    .any(|component| component.represents::<ChildOf>())
            })
            .or_else(|| self.snapshot.entities.first())
            .map(|dynamic| dynamic.entity);
        let mut map = EntityHashMap::default();
        if let Some(root) = root {
            map.insert(root, self.entity);
        }
        self.snapshot.write_to_world(world, &mut map).ok();
        state.touch();
    }
}

/// Renames an entity. Consecutive renames of the same entity collapse into one step.
pub struct Rename {
    /// Entity being renamed.
    pub entity: Entity,
    /// Name before the edit.
    pub before: String,
    /// Name after the edit.
    pub after: String,
}

impl Op<DocState> for Rename {
    fn name(&self) -> &str {
        "rename entity"
    }

    fn apply(&mut self, state: &mut DocState) {
        if let Some(mut name) = state.world_mut().get_mut::<Name>(self.entity) {
            name.set(self.after.clone());
        }
        state.touch();
    }

    fn revert(&mut self, state: &mut DocState) {
        if let Some(mut name) = state.world_mut().get_mut::<Name>(self.entity) {
            name.set(self.before.clone());
        }
        state.touch();
    }

    fn try_merge(&mut self, next: &dyn Op<DocState>) -> bool {
        let Some(next) = next.as_any().and_then(|any| any.downcast_ref::<Self>()) else {
            return false;
        };
        if next.entity != self.entity {
            return false;
        }
        self.after.clone_from(&next.after);
        true
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

/// Moves an entity. Consecutive moves of the same entity collapse into one step.
pub struct SetTranslation {
    /// Entity being moved.
    pub entity: Entity,
    /// Translation before the edit.
    pub before: Vec3,
    /// Translation after the edit.
    pub after: Vec3,
}

impl Op<DocState> for SetTranslation {
    fn name(&self) -> &str {
        "move entity"
    }

    fn apply(&mut self, state: &mut DocState) {
        put(state.world_mut(), self.entity, self.after);
        state.touch();
    }

    fn revert(&mut self, state: &mut DocState) {
        put(state.world_mut(), self.entity, self.before);
        state.touch();
    }

    fn try_merge(&mut self, next: &dyn Op<DocState>) -> bool {
        let Some(next) = next.as_any().and_then(|any| any.downcast_ref::<Self>()) else {
            return false;
        };
        if next.entity != self.entity {
            return false;
        }
        self.after = next.after;
        true
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

fn put(world: &mut World, entity: Entity, translation: Vec3) {
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = translation;
    }
}

fn subtree(world: &World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut seen = EntityHashSet::default();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if !seen.insert(entity) {
            continue;
        }
        out.push(entity);
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter().copied());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_editor_core::History;

    #[test]
    fn spawn_then_undo_removes_and_redo_restores() {
        let mut doc = DocState::new_empty();
        let mut history = History::default();
        history.push(&mut doc, Box::new(SpawnEntity::new("Cube", None)));
        let spawned = doc.hierarchy()[0].entity;
        assert_eq!(doc.entity_name(spawned), Some("Cube"));

        history.undo(&mut doc);
        assert!(doc.hierarchy().is_empty());
        assert!(doc.world().get_entity(spawned).is_err());

        history.redo(&mut doc);
        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Cube");
    }

    #[test]
    fn spawn_with_parent_puts_it_under_the_parent() {
        let mut doc = DocState::new_empty();
        let parent = doc.world_mut().spawn(Name::new("Root")).id();
        let mut op = SpawnEntity::new("Child", Some(parent));
        op.apply(&mut doc);

        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].name, "Child");
        assert_eq!(rows[1].depth, 1);
    }

    #[test]
    fn despawn_then_undo_restores_name_and_components() {
        let mut doc = DocState::new_empty();
        let parent = doc
            .world_mut()
            .spawn((Name::new("Root"), Transform::from_translation(Vec3::X)))
            .id();
        let child = doc
            .world_mut()
            .spawn((Name::new("Child"), ChildOf(parent)))
            .id();

        let mut op = DespawnEntity::capture(doc.world(), parent).unwrap();
        op.apply(&mut doc);
        assert!(doc.world().get_entity(parent).is_err());
        assert!(doc.world().get_entity(child).is_err());

        op.revert(&mut doc);
        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Root");
        assert_eq!(rows[0].entity.index(), parent.index());
        assert_eq!(rows[1].name, "Child");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(
            doc.world()
                .get::<Transform>(rows[0].entity)
                .unwrap()
                .translation,
            Vec3::X
        );
        assert!(doc.hierarchy()[0].entity != parent);
    }

    #[test]
    fn three_renames_are_one_undo_step() {
        let mut doc = DocState::new_empty();
        let mut history = History::default();
        let entity = doc.world_mut().spawn(Name::new("a")).id();
        for (before, after) in [("a", "b"), ("b", "c"), ("c", "d")] {
            history.push(
                &mut doc,
                Box::new(Rename {
                    entity,
                    before: before.into(),
                    after: after.into(),
                }),
            );
        }

        assert_eq!(history.len(), 1);
        assert_eq!(doc.entity_name(entity), Some("d"));

        history.undo(&mut doc);
        assert_eq!(doc.entity_name(entity), Some("a"));
        assert!(!history.can_undo());

        history.redo(&mut doc);
        assert_eq!(doc.entity_name(entity), Some("d"));
    }

    #[test]
    fn renames_of_different_entities_do_not_merge() {
        let mut doc = DocState::new_empty();
        let first = doc.world_mut().spawn(Name::new("a")).id();
        let second = doc.world_mut().spawn(Name::new("b")).id();
        let mut history = History::default();
        history.push(
            &mut doc,
            Box::new(Rename {
                entity: first,
                before: "a".into(),
                after: "c".into(),
            }),
        );
        history.push(
            &mut doc,
            Box::new(Rename {
                entity: second,
                before: "b".into(),
                after: "d".into(),
            }),
        );
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn three_translations_are_one_undo_step() {
        let mut doc = DocState::new_empty();
        let mut history = History::default();
        let entity = doc
            .world_mut()
            .spawn((Name::new("mover"), Transform::default()))
            .id();
        for x in 1..=3 {
            history.push(
                &mut doc,
                Box::new(SetTranslation {
                    entity,
                    before: Vec3::new(x as f32 - 1.0, 0.0, 0.0),
                    after: Vec3::new(x as f32, 0.0, 0.0),
                }),
            );
        }

        assert_eq!(history.len(), 1);
        assert_eq!(
            doc.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(3.0, 0.0, 0.0)
        );

        history.undo(&mut doc);
        assert_eq!(
            doc.world().get::<Transform>(entity).unwrap().translation,
            Vec3::ZERO
        );
        assert!(!history.can_undo());

        history.redo(&mut doc);
        assert_eq!(
            doc.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(3.0, 0.0, 0.0)
        );
    }

    #[test]
    fn translation_of_missing_transform_is_a_no_op() {
        let mut doc = DocState::new_empty();
        let entity = doc.world_mut().spawn(Name::new("naked")).id();
        let mut op = SetTranslation {
            entity,
            before: Vec3::ONE,
            after: Vec3::ZERO,
        };
        op.apply(&mut doc);
        assert!(doc.world().get::<Transform>(entity).is_none());
    }
}
