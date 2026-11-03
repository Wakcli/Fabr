use std::{
    fs,
    path::{Path, PathBuf},
};

use eme_app::{App, TaskPoolPlugin};
use eme_asset::{AssetPlugin, AssetServer};
use eme_ecs::{
    component::Component,
    entity::{Entity, EntityHashMap, EntityHashSet},
    hierarchy::{ChildOf, Children},
    name::Name,
    reflect::{AppTypeRegistry, ReflectComponent},
    resource::IS_RESOURCE,
    world::World,
};
use eme_math::{Quat, Vec3};
use eme_reflect::Reflect;
use eme_transform::prelude::Transform;
use eme_world_serialization::{serde::WorldDeserializer, DynamicWorld};
use serde::de::DeserializeSeed;

use crate::DocError;

/// One row of the hierarchy panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeItem {
    /// Entity this row shows.
    pub entity: Entity,
    /// `Name` of the entity, or its debug id when it has none.
    pub name: String,
    /// Nesting level, zero for roots.
    pub depth: usize,
}

/// Stable sibling order for the hierarchy panel, persisted with the scene.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[reflect(Component)]
pub struct HierarchyOrder(pub u64);

/// One open scene: the world being edited, its file, its dirty flag, and the play snapshot.
///
/// The world is built from a bare [`App`] carrying only the task pools and the asset plugin, so it
/// holds `AppTypeRegistry` and `AssetServer` and nothing else. Registered for reflection:
/// `Name`, `Entity`, `Transform`, `Vec3`, `Quat`, `ChildOf`, `Children`. `GlobalTransform` is left
/// unregistered so derived transforms are never written to the file.
pub struct DocState {
    pub(crate) world: World,
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty: bool,
    pub(crate) play: Option<String>,
    pub(crate) next_order: u64,
}

impl DocState {
    /// An empty, unsaved document.
    pub fn new_empty() -> Self {
        Self {
            world: bootstrap(),
            path: None,
            dirty: false,
            play: None,
            next_order: 0,
        }
    }

    /// Opens a RON scene file into a new document.
    pub fn open(path: &Path) -> Result<Self, DocError> {
        let src = fs::read_to_string(path)?;
        let mut doc = Self::new_empty();
        doc.world = doc.decode(&src)?;
        doc.path = Some(path.to_path_buf());
        doc.next_order = doc
            .world
            .iter_entities()
            .filter_map(|entity| entity.get::<HierarchyOrder>())
            .map(|order| order.0 + 1)
            .max()
            .unwrap_or(0);
        Ok(doc)
    }

    /// Writes the document back to its own path.
    pub fn save(&mut self) -> Result<(), DocError> {
        let path = self.path.clone().ok_or(DocError::NoPath)?;
        self.save_as(&path)
    }

    /// Writes the document to `path` and adopts it as the document path.
    pub fn save_as(&mut self, path: &Path) -> Result<(), DocError> {
        self.ensure_order();
        let src = self.encode()?;
        fs::write(path, src)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// Whether the document differs from the file on disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Path of the file backing this document, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Clears the dirty flag without touching the file.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// The world being edited.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// The world being edited.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// Flattened hierarchy: tree order by `Children`, roots in spawn order.
    pub fn hierarchy(&self) -> Vec<TreeItem> {
        let mut entries: Vec<(Entity, Option<Entity>, u64)> = self
            .world
            .iter_entities()
            .filter(|entity| !entity.contains_id(IS_RESOURCE))
            .map(|entity| {
                let order = entity
                    .get::<HierarchyOrder>()
                    .map_or(u64::MAX, |order| order.0);
                (
                    entity.id(),
                    entity.get::<ChildOf>().map(ChildOf::parent),
                    order,
                )
            })
            .collect();
        entries.sort_by_key(|(entity, _, order)| (*order, entity.index_u32()));

        let mut seen = EntityHashSet::default();
        let mut items = Vec::with_capacity(entries.len());
        for (entity, parent, _) in &entries {
            if parent.is_none() {
                walk(&self.world, *entity, 0, &mut seen, &mut items);
            }
        }
        for (entity, _, _) in &entries {
            walk(&self.world, *entity, 0, &mut seen, &mut items);
        }
        items
    }

    /// `Name` of `entity`, or `None` when the entity is gone or unnamed.
    pub fn entity_name(&self, entity: Entity) -> Option<&str> {
        self.world.get::<Name>(entity).map(Name::as_str)
    }

    pub(crate) fn touch(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn ensure_order(&mut self) {
        let mut ids: Vec<Entity> = self
            .world
            .iter_entities()
            .filter(|entity| !entity.contains_id(IS_RESOURCE))
            .filter(|entity| entity.get::<HierarchyOrder>().is_none())
            .map(|entity| entity.id())
            .collect();
        ids.sort_by_key(|id| id.index_u32());
        for id in ids {
            self.world
                .entity_mut(id)
                .insert(HierarchyOrder(self.next_order));
            self.next_order += 1;
        }
    }

    pub(crate) fn encode(&self) -> Result<String, DocError> {
        let registry = self.world.resource::<AppTypeRegistry>().read();
        let world = DynamicWorld::from_world_with(&self.world, &registry);
        Ok(world.serialize(&registry)?)
    }

    pub(crate) fn decode(&self, src: &str) -> Result<World, DocError> {
        let world = {
            let registry = self.world.resource::<AppTypeRegistry>().read();
            let mut server = self.world.resource::<AssetServer>().clone();
            let mut de = ron::de::Deserializer::from_str(src)?;
            let decoded = WorldDeserializer {
                type_registry: &registry,
                load_from_path: &mut server,
            }
            .deserialize(&mut de)?;
            let mut world = self.blank();
            decoded.write_to_world(&mut world, &mut EntityHashMap::default())?;
            world
        };
        Ok(world)
    }

    fn blank(&self) -> World {
        let mut world = World::new();
        world.insert_resource(self.world.resource::<AppTypeRegistry>().clone());
        world.insert_resource(self.world.resource::<AssetServer>().clone());
        world
    }
}

fn walk(
    world: &World,
    entity: Entity,
    depth: usize,
    seen: &mut EntityHashSet,
    out: &mut Vec<TreeItem>,
) {
    if !seen.insert(entity) {
        return;
    }
    out.push(TreeItem {
        entity,
        name: name_of(world, entity),
        depth,
    });
    if let Some(children) = world.get::<Children>(entity) {
        for &child in children.iter() {
            walk(world, child, depth + 1, seen, out);
        }
    }
}

fn name_of(world: &World, entity: Entity) -> String {
    world
        .get::<Name>(entity)
        .map_or_else(|| format!("{entity:?}"), |name| name.as_str().to_owned())
}

fn bootstrap() -> World {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
    let mut world = core::mem::take(app.world_mut());
    register(&mut world);
    world
}

fn register(world: &mut World) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let mut registry = registry.write();
    registry.register::<Name>();
    registry.register::<Entity>();
    registry.register::<Transform>();
    registry.register::<Vec3>();
    registry.register::<Quat>();
    registry.register::<ChildOf>();
    registry.register::<Children>();
    registry.register::<HierarchyOrder>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme_editor_doc_{}_{tag}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> DocState {
        let mut doc = DocState::new_empty();
        let root = doc
            .world_mut()
            .spawn((Name::new("Root"), Transform::default()))
            .id();
        doc.world_mut()
            .spawn((Name::new("Child"), Transform::default(), ChildOf(root)));
        doc.world_mut().spawn(Name::new("Other"));
        doc
    }

    #[test]
    fn hierarchy_is_tree_ordered_with_depths() {
        let doc = sample();
        let rows: Vec<(String, usize)> = doc
            .hierarchy()
            .into_iter()
            .map(|item| (item.name, item.depth))
            .collect();
        assert_eq!(
            rows,
            [
                ("Root".to_string(), 0),
                ("Child".to_string(), 1),
                ("Other".to_string(), 0),
            ]
        );
    }

    #[test]
    fn hierarchy_falls_back_to_entity_debug() {
        let mut doc = DocState::new_empty();
        let entity = doc.world_mut().spawn_empty().id();
        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, format!("{entity:?}"));
        assert_eq!(doc.entity_name(entity), None);
    }

    #[test]
    fn hierarchy_skips_resource_entities() {
        let doc = sample();
        let rows = doc.hierarchy();
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn save_then_open_keeps_hierarchy() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("scene.scn.ron");
        let mut doc = sample();
        assert!(!doc.is_dirty());

        doc.save_as(&path).unwrap();
        assert_eq!(doc.path(), Some(path.as_path()));
        assert!(!doc.is_dirty());

        let opened = DocState::open(&path).unwrap();
        let before: Vec<(String, usize)> = doc
            .hierarchy()
            .into_iter()
            .map(|item| (item.name, item.depth))
            .collect();
        let after: Vec<(String, usize)> = opened
            .hierarchy()
            .into_iter()
            .map(|item| (item.name, item.depth))
            .collect();
        assert_eq!(before, after);
        assert_eq!(
            opened.entity_name(opened.hierarchy()[1].entity),
            Some("Child")
        );
        assert!(!opened.is_dirty());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_without_path_errors() {
        let mut doc = DocState::new_empty();
        assert!(matches!(doc.save(), Err(DocError::NoPath)));
    }

    #[test]
    fn mark_clean_clears_dirty() {
        let mut doc = DocState::new_empty();
        doc.touch();
        assert!(doc.is_dirty());
        doc.mark_clean();
        assert!(!doc.is_dirty());
    }
}
