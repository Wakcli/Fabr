//! Selection feedback for the viewport: an inverted hull drawn around the
//! selected object instead of recoloring its material.

use std::collections::HashSet;

use eme::light::NotShadowCaster;
use eme::mesh::VertexAttributeValues;
use eme::prelude::*;
use eme::render::render_resource::Face;

use crate::orbit::ViewMode;
use crate::scene::SceneState;

/// Marks a hull entity that draws the outline of its selected parent.
#[derive(Component, Clone, Copy, Default)]
pub struct SelectionOutline;

/// The shared unlit material every selection hull draws with.
#[derive(Resource, Clone)]
pub struct OutlineMaterial(pub Handle<StandardMaterial>);

/// Local scale limit for tiny objects, so a hairline mesh cannot explode its hull.
const HULL_SCALE_MAX: f32 = 4.0;
/// World-space distance the hull sits outside the mesh it wraps.
const HULL_GAP: f32 = 0.02;
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
    view: Option<Res<ViewMode>>,
    outline: Option<Res<OutlineMaterial>>,
    meshes: Query<&Mesh3d, Without<SelectionOutline>>,
    assets: Res<Assets<Mesh>>,
    worlds: Query<&GlobalTransform>,
    mut hulls: Query<(Entity, &ChildOf, &mut Mesh3d, &mut Transform), With<SelectionOutline>>,
    mut wanted: Local<HashSet<Entity>>,
    mut covered: Local<HashSet<Entity>>,
) {
    let Some(outline) = outline else {
        return;
    };

    wanted.clear();
    for id in state.selected_ids().filter(|_| !matches!(view.as_deref(), Some(ViewMode::Game))) {
        if let Some(&entity) = state.entities.get(&id) {
            wanted.insert(entity);
        }
    }

    covered.clear();
    for (hull, child_of, mut drawn, mut shell) in &mut hulls {
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
        let want = hull_scale(&mesh.0, &assets, worlds.get(parent).ok());
        if shell.scale != Vec3::splat(want) {
            shell.scale = Vec3::splat(want);
        }
    }

    for parent in wanted.iter().copied().filter(|it| !covered.contains(it)) {
        let Ok(mesh) = meshes.get(parent) else {
            continue;
        };
        let scale = hull_scale(&mesh.0, &assets, worlds.get(parent).ok());
        commands.spawn((
            SelectionOutline,
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline.0.clone()),
            Transform::from_scale(Vec3::splat(scale)),
            Pickable::IGNORE,
            NotShadowCaster,
            ChildOf(parent),
        ));
    }
}

/// Hull scale that puts the shell [`HULL_GAP`] world units outside the mesh,
/// whatever the object's own scale is.
fn hull_scale(mesh: &Handle<Mesh>, assets: &Assets<Mesh>, world: Option<&GlobalTransform>) -> f32 {
    let radius = assets.get(mesh).and_then(mesh_radius).unwrap_or(0.0);
    let parent = world
        .map(|transform| transform.scale().abs().max_element())
        .unwrap_or(1.0);
    if radius <= f32::EPSILON || parent <= f32::EPSILON {
        return 1.0 + HULL_GAP;
    }
    (1.0 + HULL_GAP / (radius * parent)).min(HULL_SCALE_MAX)
}

/// Distance from the mesh origin to its farthest vertex, in local space.
fn mesh_radius(mesh: &Mesh) -> Option<f32> {
    let VertexAttributeValues::Float32x3(points) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)? else {
        return None;
    };
    points
        .iter()
        .map(|point| Vec3::from_array(*point).length())
        .reduce(f32::max)
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

    #[test]
    fn hull_offset_stays_constant_when_the_object_scales() {
        let mut app = app();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::default());
        let assets = app.world().resource::<Assets<Mesh>>();
        let unit = GlobalTransform::default();
        let big = GlobalTransform::from_scale(Vec3::splat(4.0));

        let small = hull_scale(&mesh, assets, Some(&unit));
        let large = hull_scale(&mesh, assets, Some(&big));
        let radius = mesh_radius(assets.get(&mesh).unwrap()).unwrap();

        assert!((radius * (small - 1.0) - HULL_GAP).abs() < 1e-5);
        assert!((radius * 4.0 * (large - 1.0) - HULL_GAP).abs() < 1e-5);
        assert!(large < small, "a bigger object needs a smaller local hull scale");
    }

    #[test]
    fn hull_offset_survives_a_missing_world_transform() {
        let mut app = app();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::default());
        let assets = app.world().resource::<Assets<Mesh>>();
        let unscaled = hull_scale(&mesh, assets, None);
        let unit = hull_scale(&mesh, assets, Some(&GlobalTransform::default()));
        assert!((unscaled - unit).abs() < 1e-6);
    }
}
