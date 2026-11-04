//! Selection feedback for the viewport: an inverted hull drawn around the
//! selected object instead of recoloring its material.

use std::collections::HashSet;

use eme::light::NotShadowCaster;
use eme::prelude::*;
use eme::render::render_resource::Face;

use crate::scene::SceneState;

/// Marks a hull entity that draws the outline of its selected parent.
#[derive(Component, Clone, Copy, Default)]
pub struct SelectionOutline;

/// The shared unlit material every selection hull draws with.
#[derive(Resource, Clone)]
pub struct OutlineMaterial(pub Handle<StandardMaterial>);

/// Local scale of a hull relative to the mesh it wraps.
const HULL_SCALE: f32 = 1.04;
/// Selection orange.
const HULL_COLOR: Color = Color::srgb(1.0, 0.62, 0.16);

/// Creates the outline material once, before any scene object exists.
pub fn init_outline_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let handle = materials.add(StandardMaterial {
        base_color: HULL_COLOR,
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });
    commands.insert_resource(OutlineMaterial(handle));
}

/// Keeps one hull child on every selected mesh object.
///
/// Objects without a [`Mesh3d`] are skipped, so lights, cameras and empties get
/// no outline. Hulls carry [`Pickable::IGNORE`] and no `SceneLink`, so picking,
/// the hierarchy and serialization never see them.
pub fn sync_outlines(
    mut commands: Commands,
    state: Res<SceneState>,
    outline: Option<Res<OutlineMaterial>>,
    meshes: Query<&Mesh3d, Without<SelectionOutline>>,
    mut hulls: Query<(Entity, &ChildOf, &mut Mesh3d), With<SelectionOutline>>,
    mut wanted: Local<HashSet<Entity>>,
    mut covered: Local<HashSet<Entity>>,
) {
    let Some(outline) = outline else {
        return;
    };

    wanted.clear();
    for id in state.selected_ids() {
        if let Some(&entity) = state.entities.get(&id) {
            wanted.insert(entity);
        }
    }

    covered.clear();
    for (hull, child_of, mut drawn) in &mut hulls {
        let parent = child_of.parent();
        let Ok(mesh) = meshes.get(parent) else {
            commands.entity(hull).despawn();
            continue;
        };
        if !wanted.contains(&parent) || !covered.insert(parent) {
            commands.entity(hull).despawn();
            continue;
        }
        if drawn.0 != mesh.0 {
            drawn.0 = mesh.0.clone();
        }
    }

    for parent in wanted.iter().copied().filter(|it| !covered.contains(it)) {
        let Ok(mesh) = meshes.get(parent) else {
            continue;
        };
        commands.spawn((
            SelectionOutline,
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline.0.clone()),
            Transform::from_scale(Vec3::splat(HULL_SCALE)),
            Pickable::IGNORE,
            NotShadowCaster,
            ChildOf(parent),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneLink;
    use eme_game_scene::ObjectKind;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<SceneState>()
            .add_systems(Update, sync_outlines);
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        app.insert_resource(OutlineMaterial(material));
        app
    }

    fn spawn_cube(app: &mut App) -> u64 {
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::default());
        let entity = app
            .world_mut()
            .spawn((Mesh3d(mesh), Transform::default()))
            .id();
        let mut state = app.world_mut().resource_mut::<SceneState>();
        let id = state.spawn(ObjectKind::Cube);
        state.entities.insert(id, entity);
        id
    }

    fn hulls(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<SelectionOutline>>()
            .iter(app.world())
            .collect()
    }

    #[test]
    fn selected_mesh_gets_one_hull() {
        let mut app = app();
        let id = spawn_cube(&mut app);
        app.world_mut().resource_mut::<SceneState>().select_only(id);

        app.update();

        let hulls = hulls(&mut app);
        assert_eq!(hulls.len(), 1);
        let hull = app.world().entity(hulls[0]);
        assert!(hull.contains::<Mesh3d>());
        assert!(hull.contains::<MeshMaterial3d<StandardMaterial>>());
        assert!(hull.contains::<NotShadowCaster>());
        assert_eq!(hull.get::<Pickable>(), Some(&Pickable::IGNORE));
        assert!(!hull.contains::<SceneLink>());
    }

    #[test]
    fn deselect_removes_the_hull() {
        let mut app = app();
        let id = spawn_cube(&mut app);
        app.world_mut().resource_mut::<SceneState>().select_only(id);
        app.update();
        assert_eq!(hulls(&mut app).len(), 1);

        app.world_mut()
            .resource_mut::<SceneState>()
            .clear_selection();
        app.update();

        assert!(hulls(&mut app).is_empty());
    }

    #[test]
    fn despawning_the_parent_removes_the_hull() {
        let mut app = app();
        let id = spawn_cube(&mut app);
        app.world_mut().resource_mut::<SceneState>().select_only(id);
        app.update();
        assert_eq!(hulls(&mut app).len(), 1);

        let parent = app.world().resource::<SceneState>().entities[&id];
        app.world_mut().despawn(parent);

        assert!(hulls(&mut app).is_empty());
    }

    #[test]
    fn repeated_sync_never_stacks_hulls() {
        let mut app = app();
        let id = spawn_cube(&mut app);
        app.world_mut().resource_mut::<SceneState>().select_only(id);

        app.update();
        app.update();
        app.update();

        assert_eq!(hulls(&mut app).len(), 1);
    }

    #[test]
    fn hull_follows_the_parent_mesh_handle() {
        let mut app = app();
        let id = spawn_cube(&mut app);
        app.world_mut().resource_mut::<SceneState>().select_only(id);
        app.update();

        let parent = app.world().resource::<SceneState>().entities[&id];
        let next = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Sphere::default().mesh().ico(2).unwrap());
        app.world_mut()
            .entity_mut(parent)
            .insert(Mesh3d(next.clone()));
        app.update();

        let hull = hulls(&mut app)[0];
        let drawn = app.world().entity(hull).get::<Mesh3d>().unwrap();
        assert_eq!(drawn.0, next);
    }

    #[test]
    fn unmeshed_selection_gets_no_hull() {
        let mut app = app();
        let entity = app.world_mut().spawn(Transform::default()).id();
        let mut state = app.world_mut().resource_mut::<SceneState>();
        let id = state.spawn(ObjectKind::PointLight);
        state.entities.insert(id, entity);
        state.select_only(id);

        app.update();

        assert!(hulls(&mut app).is_empty());
    }
}
