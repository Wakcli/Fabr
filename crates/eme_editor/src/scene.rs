//! Viewport scene state: the scene document, the entities that mirror it, and
//! the reconcile/spawn/selection systems shared by the viewport and the docks.

use std::collections::HashMap;

use eme::color::LinearRgba;
use eme::feathers::theme::UiTheme;
use eme::prelude::*;
use eme::render::render_resource::Face;
use eme_game_scene::{ObjectKind, ObjectTransform, SceneDoc, SceneObject};

use crate::orbit::{FlyMove, ViewMode};
use crate::tokens;

/// Distance a spawned object advances along one grid cell.
pub const CELL: f32 = 2.0;
/// How far the arrow keys nudge the selected object.
pub const MOVE_STEP: f32 = 0.25;
/// Cube edge length in world units.
const CUBE: f32 = 1.0;
/// Sphere radius, cylinder radius and height.
const SPHERE_R: f32 = 0.5;
const CYL_R: f32 = 0.5;
const CYL_H: f32 = 1.0;
/// Plane plate size; the mesh has no thickness so it floats just off the grid.
const PLANE: f32 = 2.0;
const PLANE_Y: f32 = 0.02;
/// Camera glyph quad, sized to read cleanly at the default view distance.
const GLYPH: (f32, f32, f32) = (1.15, 1.15, 0.01);
const CAMERA_TEX: &str = "editor/icons/camera_glyph.png";
/// Camera frustum guide drawn from the object's own rotation.
const FRUSTUM_DEPTH: f32 = 1.15;
const FRUSTUM_HALF_H: f32 = 0.30;
const FRUSTUM_HALF_W: f32 = FRUSTUM_HALF_H * 16.0 / 9.0;
/// Neutral color of an unselected camera frustum.
const FRUSTUM_DIM: Color = Color::srgba(0.62, 0.66, 0.72, 0.45);
/// Accent strength applied to a selected object.
const SELECT_GLOW: f32 = 0.35;

/// Marks the viewport's 3D camera so observers can resolve pointer rays.
#[derive(Component, Clone, Copy, Default)]
pub struct ViewportCamera;

/// Marks the billboarded camera icon.
#[derive(Component, Clone, Copy, Default)]
pub struct CameraGlyph;

/// Marks the root of a camera object, which owns its orientation.
#[derive(Component, Clone, Copy, Default)]
pub struct CameraRoot;

/// Ties a free-standing camera glyph to the camera object it represents.
#[derive(Component, Clone, Copy)]
pub struct GlyphOwner(pub Entity);

/// Copies the viewport camera's rotation so a glyph always faces the user.
#[derive(Component, Clone, Copy, Default)]
pub struct Billboard;

/// Ties a spawned entity back to its document object.
#[derive(Component, Clone, Copy)]
pub struct SceneLink {
    pub id: u64,
    pub kind: ObjectKind,
}

impl Default for SceneLink {
    fn default() -> Self {
        Self {
            id: 0,
            kind: ObjectKind::Empty,
        }
    }
}

/// Asks the editor UI to open the object menu for a viewport right click.
#[derive(Event, Clone, Copy)]
pub struct ObjectMenu {
    pub id: u64,
    pub pos: Vec2,
}

/// Kinds whose mesh takes the selection accent. The camera glyph is textured,
/// so tinting its base color would wash the icon out.
fn tintable(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Cube | ObjectKind::Sphere | ObjectKind::Cylinder | ObjectKind::Plane
    )
}

/// The authored document plus the entity mirror the viewport draws.
#[derive(Resource, Default)]
pub struct SceneState {
    /// Source of truth: every edit lands here first.
    pub doc: SceneDoc,
    /// Entities mirroring `doc`, keyed by object id.
    pub entities: HashMap<u64, Entity>,
    pub selected: Option<u64>,
    /// Bumped on structural changes so UI sync systems can skip unchanged frames.
    pub rev: u64,
    next_slot: usize,
    counts: [u32; 8],
}

impl SceneState {
    /// Sets the selection and invalidates the scene UI when it actually changes.
    pub fn select(&mut self, target: Option<u64>) {
        if self.selected == target {
            return;
        }
        self.selected = target;
        self.touch();
    }

    /// Invalidates the scene UI.
    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// The tracked object for `id`, if it is a scene object.
    pub fn object(&self, id: u64) -> Option<&SceneObject> {
        self.doc.get(id)
    }

    pub fn selected_object(&self) -> Option<&SceneObject> {
        self.selected.and_then(|id| self.doc.get(id))
    }

    /// Spawns a new object of `kind` on the next free grid cell.
    pub fn spawn(&mut self, kind: ObjectKind) -> u64 {
        let (x, z) = cell(self.take_slot());
        let transform = resting(kind, x, z);
        let name = self.next_name(kind);
        self.spawn_at(kind, transform, name)
    }

    /// Spawns an object at an explicit transform, as a paste needs.
    pub fn spawn_at(
        &mut self,
        kind: ObjectKind,
        transform: ObjectTransform,
        name: impl Into<String>,
    ) -> u64 {
        let id = self.doc.add(kind, name, transform);
        self.touch();
        id
    }

    /// Copies an object one unit along X and returns the new id.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let new_id = self.doc.duplicate(id)?;
        self.touch();
        Some(new_id)
    }

    /// Removes an object, clearing the selection when it was the selected one.
    pub fn despawn(&mut self, id: u64) -> bool {
        if self.doc.remove(id).is_none() {
            return false;
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.touch();
        true
    }

    /// Replaces the document, as a scene load does, and resyncs naming state.
    pub fn replace_doc(&mut self, doc: SceneDoc) {
        self.doc = doc;
        self.selected = self.selected.filter(|id| self.doc.get(*id).is_some());
        self.resync_counts();
        self.touch();
    }

    fn resync_counts(&mut self) {
        let mut counts = [0u32; 8];
        for obj in &self.doc.objects {
            counts[obj.kind.index()] += 1;
        }
        self.counts = counts;
        self.next_slot = self.doc.len();
    }

    fn take_slot(&mut self) -> usize {
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }

    fn next_name(&mut self, kind: ObjectKind) -> String {
        let slot = kind.index();
        loop {
            self.counts[slot] += 1;
            let name = format!("{} {}", kind.label(), self.counts[slot]);
            if self.doc.objects.iter().all(|obj| obj.name != name) {
                return name;
            }
        }
    }
}

/// Reusable meshes and the shared camera glyph material.
#[derive(Resource, Default)]
pub struct SceneAssets {
    cube: Option<Handle<Mesh>>,
    sphere: Option<Handle<Mesh>>,
    cylinder: Option<Handle<Mesh>>,
    plane: Option<Handle<Mesh>>,
    glyph: Option<Handle<Mesh>>,
    glyph_material: Option<Handle<StandardMaterial>>,
}

impl SceneAssets {
    fn cube(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.cube
            .get_or_insert_with(|| meshes.add(Cuboid::new(CUBE, CUBE, CUBE)))
            .clone()
    }

    fn sphere(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.sphere
            .get_or_insert_with(|| meshes.add(Sphere::new(SPHERE_R).mesh().ico(4).unwrap()))
            .clone()
    }

    fn cylinder(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.cylinder
            .get_or_insert_with(|| meshes.add(Cylinder::new(CYL_R, CYL_H)))
            .clone()
    }

    fn plane(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.plane
            .get_or_insert_with(|| meshes.add(Plane3d::default().mesh().size(PLANE, PLANE)))
            .clone()
    }

    fn glyph(&mut self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.glyph
            .get_or_insert_with(|| meshes.add(Cuboid::new(GLYPH.0, GLYPH.1, GLYPH.2)))
            .clone()
    }

    fn glyph_material(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        asset_server: &AssetServer,
    ) -> Handle<StandardMaterial> {
        self.glyph_material
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(asset_server.load(CAMERA_TEX)),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: Some(Face::Back),
                    ..default()
                })
            })
            .clone()
    }
}

/// Grid cell for a spawn slot; slots never repeat, so objects never overlap.
pub fn cell(slot: usize) -> (f32, f32) {
    let x = (slot % 2) as f32 * CELL - CELL / 2.0;
    let z = (slot / 2) as f32 * CELL - CELL / 2.0;
    (x, z)
}

/// The resting transform for a freshly spawned kind.
fn resting(kind: ObjectKind, x: f32, z: f32) -> ObjectTransform {
    let y = match kind {
        ObjectKind::Empty => 0.0,
        ObjectKind::Cube | ObjectKind::Sphere | ObjectKind::Cylinder => CUBE / 2.0,
        ObjectKind::Plane => PLANE_Y,
        ObjectKind::PointLight => 3.0,
        ObjectKind::Sun => 4.0,
        ObjectKind::Camera => 1.8,
    };
    let transform = ObjectTransform::from_translation([x, y, z]);
    if kind == ObjectKind::Sun {
        let rotation = Quat::from_rotation_x(-0.9) * Quat::from_rotation_y(0.6);
        return transform.with_rotation(rotation.to_array());
    }
    transform
}

fn to_transform(transform: &ObjectTransform) -> Transform {
    Transform::from_xyz(
        transform.translation[0],
        transform.translation[1],
        transform.translation[2],
    )
    .with_rotation(Quat::from_array(transform.rotation))
    .with_scale(Vec3::from_array(transform.scale))
}

/// Brings the entity mirror in line with the document after a `rev` change.
pub fn reconcile_scene(
    mut commands: Commands,
    state: ResMut<SceneState>,
    mut assets: ResMut<SceneAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    theme: Res<UiTheme>,
    links: Query<&SceneLink>,
    glyphs: Query<(Entity, &GlyphOwner)>,
) {
    let state = state.into_inner();
    let ids: Vec<u64> = state.doc.objects.iter().map(|obj| obj.id).collect();

    let stale: Vec<(u64, Entity)> = state
        .entities
        .iter()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale {
        for (glyph, owner) in &glyphs {
            if owner.0 == entity {
                commands.entity(glyph).despawn();
            }
        }
        commands.entity(entity).despawn();
        state.entities.remove(&id);
    }

    for id in ids {
        let Some(obj) = state.doc.get(id) else {
            continue;
        };
        let kind = obj.kind;
        let name = obj.name.clone();
        let transform = obj.transform();
        let color = obj.color;

        match state.entities.get(&id).copied() {
            Some(entity) if links.get(entity).is_ok_and(|link| link.kind == kind) => {
                commands
                    .entity(entity)
                    .insert((to_transform(&transform), Name::new(name)));
            }
            Some(entity) => {
                commands.entity(entity).despawn();
                state.entities.remove(&id);
                let entity = spawn_entity(
                    &mut commands,
                    &mut assets,
                    &mut meshes,
                    &mut materials,
                    &asset_server,
                    &theme,
                    id,
                    kind,
                    name,
                    &transform,
                    color,
                );
                state.entities.insert(id, entity);
            }
            None => {
                let entity = spawn_entity(
                    &mut commands,
                    &mut assets,
                    &mut meshes,
                    &mut materials,
                    &asset_server,
                    &theme,
                    id,
                    kind,
                    name,
                    &transform,
                    color,
                );
                state.entities.insert(id, entity);
            }
        }
    }
}

fn spawn_entity(
    commands: &mut Commands,
    assets: &mut SceneAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    theme: &UiTheme,
    id: u64,
    kind: ObjectKind,
    name: String,
    transform: &ObjectTransform,
    color: Option<[f32; 4]>,
) -> Entity {
    let trs = to_transform(transform);
    let entity = match kind {
        ObjectKind::Empty => commands.spawn(trs).id(),
        ObjectKind::PointLight => commands
            .spawn((
                PointLight {
                    intensity: 800_000.0,
                    range: 25.0,
                    ..default()
                },
                trs,
            ))
            .id(),
        // the viewport already owns the shadow-casting sun, so object suns
        // light without a second set of shadow cascades
        ObjectKind::Sun => commands
            .spawn((
                DirectionalLight {
                    illuminance: 8_000.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                trs,
            ))
            .id(),
        ObjectKind::Camera => {
            let entity = commands
                .spawn((trs, SceneLink { id, kind }, CameraRoot))
                .id();
            let mesh = assets.glyph(meshes);
            let material = assets.glyph_material(materials, asset_server);
            let glyph_trs = Transform::from_translation(trs.translation);
            commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    glyph_trs,
                    Billboard,
                    CameraGlyph,
                    GlyphOwner(entity),
                ))
                .insert(SceneLink { id, kind })
                .observe(click_object)
                .observe(drag_start)
                .observe(drag_object);
            entity
        }
        ObjectKind::Cube | ObjectKind::Sphere | ObjectKind::Cylinder | ObjectKind::Plane => {
            let mesh = match kind {
                ObjectKind::Cube => assets.cube(meshes),
                ObjectKind::Sphere => assets.sphere(meshes),
                ObjectKind::Cylinder => assets.cylinder(meshes),
                _ => assets.plane(meshes),
            };
            let base = color.map_or_else(
                || theme.color(&tokens::TEXT),
                |[r, g, b, a]| Color::linear_rgba(r, g, b, a),
            );
            let mut material = StandardMaterial::from(base);
            material.perceptual_roughness = 0.55;
            material.metallic = 0.0;
            commands
                .spawn((Mesh3d(mesh), MeshMaterial3d(materials.add(material)), trs))
                .id()
        }
    };

    commands
        .entity(entity)
        .insert((Name::new(name), SceneLink { id, kind }))
        .observe(click_object)
        .observe(drag_start)
        .observe(drag_object);
    entity
}

/// Paints the accent onto the selected object.
pub fn tint_selection(
    state: Res<SceneState>,
    theme: Res<UiTheme>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    let base = theme.color(&tokens::TEXT);
    let accent = theme.color(&tokens::ACCENT);
    let linear = accent.to_linear();
    let glow = LinearRgba::rgb(
        linear.red * SELECT_GLOW,
        linear.green * SELECT_GLOW,
        linear.blue * SELECT_GLOW,
    );

    for obj in &state.doc.objects {
        if !tintable(obj.kind) {
            continue;
        }
        let Some(entity) = state.entities.get(&obj.id) else {
            continue;
        };
        let Ok(handle) = meshes.get(*entity) else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        let selected = state.selected == Some(obj.id);
        let color = if selected {
            accent
        } else if let Some([r, g, b, a]) = obj.color {
            Color::linear_rgba(r, g, b, a)
        } else {
            base
        };
        let emissive = if selected { glow } else { LinearRgba::BLACK };
        if material.base_color != color {
            material.base_color = color;
        }
        if material.emissive != emissive {
            material.emissive = emissive;
        }
    }
}

/// Drives free-standing camera icons from their object's pose, ignoring object scale.
pub fn face_viewport_camera(
    cams: Query<&GlobalTransform, With<ViewportCamera>>,
    owners: Query<&GlobalTransform>,
    mut glyphs: Query<(&GlyphOwner, &mut Transform), With<Billboard>>,
) {
    let Ok(cam) = cams.single() else {
        return;
    };
    let (_, facing, _) = cam.to_scale_rotation_translation();
    let want_rotation = facing * Quat::from_rotation_y(std::f32::consts::PI);
    for (owner, mut transform) in &mut glyphs {
        let Ok(source) = owners.get(owner.0) else {
            continue;
        };
        transform.translation = source.translation();
        transform.rotation = want_rotation;
        transform.scale = Vec3::ONE;
    }
}

/// Draws the frustum a camera object looks through, following its own rotation.
pub fn draw_camera_frusta(
    state: Res<SceneState>,
    theme: Res<UiTheme>,
    view: Res<ViewMode>,
    cameras: Query<(&SceneLink, &GlobalTransform), With<CameraRoot>>,
    mut gizmos: Gizmos,
) {
    if *view == ViewMode::Game {
        return;
    }
    let accent = theme.color(&tokens::ACCENT);
    for (link, global) in &cameras {
        if link.kind != ObjectKind::Camera {
            continue;
        }
        let color = if state.selected == Some(link.id) {
            accent
        } else {
            FRUSTUM_DIM
        };
        let (_, rotation, _) = global.to_scale_rotation_translation();
        let forward = rotation * Vec3::NEG_Z;
        let apex = global.translation() + forward * 0.24;
        let up = rotation * Vec3::Y;
        let right = rotation * Vec3::X;
        let center = apex + forward * FRUSTUM_DEPTH;
        let corners = [
            center + right * FRUSTUM_HALF_W + up * FRUSTUM_HALF_H,
            center - right * FRUSTUM_HALF_W + up * FRUSTUM_HALF_H,
            center - right * FRUSTUM_HALF_W - up * FRUSTUM_HALF_H,
            center + right * FRUSTUM_HALF_W - up * FRUSTUM_HALF_H,
        ];
        for i in 0..4 {
            gizmos.line(apex, corners[i], color);
            gizmos.line(corners[i], corners[(i + 1) % 4], color);
        }
        gizmos.line(apex, apex + forward * 0.3, color);
        let top = center + up * FRUSTUM_HALF_H;
        gizmos.line(top, top + up * 0.16, color);
    }
}

/// Arrow keys nudge the selected object on X/Z; Delete and Backspace remove it.
pub fn keyboard_edit(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SceneState>,
    mut objects: Query<&mut Transform>,
) {
    let Some(id) = state.selected else {
        return;
    };

    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        state.despawn(id);
        return;
    }

    let mut step = Vec2::ZERO;
    if keys.just_pressed(KeyCode::ArrowLeft) {
        step.x -= MOVE_STEP;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        step.x += MOVE_STEP;
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        step.y -= MOVE_STEP;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        step.y += MOVE_STEP;
    }
    if step == Vec2::ZERO {
        return;
    }

    let Some(obj) = state.doc.get(id) else {
        return;
    };
    let mut translation = obj.translation;
    translation[0] += step.x;
    translation[2] += step.y;
    state.doc.set_translation(id, translation);

    let Some(entity) = state.entities.get(&id) else {
        return;
    };
    if let Ok(mut transform) = objects.get_mut(*entity) {
        transform.translation.x = translation[0];
        transform.translation.z = translation[2];
    }
}

/// Grab point captured on drag start, so the object does not snap to the cursor.
#[derive(Component, Clone, Copy)]
struct DragGrab(Vec3);

fn ground_hit(
    cams: &Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    position: Vec2,
    height: f32,
) -> Option<Vec3> {
    let (camera, transform) = cams.single().ok()?;
    let ray = camera.viewport_to_world(transform, position).ok()?;
    ray.plane_intersection_point(Vec3::new(0.0, height, 0.0), InfinitePlane3d::new(Vec3::Y))
}

fn click_object(
    click: On<PointerClick>,
    links: Query<&SceneLink>,
    fly: Res<FlyMove>,
    mut state: ResMut<SceneState>,
    mut commands: Commands,
) {
    let Ok(link) = links.get(click.entity) else {
        return;
    };
    match click.button {
        PointerButton::Primary => state.select(Some(link.id)),
        PointerButton::Secondary => {
            if fly.suppress_menu || click.duration.as_millis() > 250 {
                return;
            }
            state.select(Some(link.id));
            commands.trigger(ObjectMenu {
                id: link.id,
                pos: click.pointer.position,
            });
        }
        _ => {}
    }
}

fn drag_start(
    start: On<PointerDragStart>,
    state: Res<SceneState>,
    links: Query<&SceneLink>,
    cams: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    objects: Query<&Transform>,
    mut commands: Commands,
) {
    let Ok(link) = links.get(start.entity) else {
        return;
    };
    if start.button != PointerButton::Primary || state.selected != Some(link.id) {
        return;
    }
    let Some(&entity) = state.entities.get(&link.id) else {
        return;
    };
    let Ok(transform) = objects.get(entity) else {
        return;
    };
    let height = transform.translation.y;
    let Some(hit) = ground_hit(&cams, start.pointer.position, height) else {
        return;
    };
    let grab = Vec3::new(
        transform.translation.x - hit.x,
        0.0,
        transform.translation.z - hit.z,
    );
    commands.entity(entity).insert(DragGrab(grab));
}

fn drag_object(
    drag: On<PointerDrag>,
    mut state: ResMut<SceneState>,
    links: Query<&SceneLink>,
    cams: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    mut objects: Query<(&mut Transform, Option<&DragGrab>)>,
) {
    let Ok(link) = links.get(drag.entity) else {
        return;
    };
    if drag.button != PointerButton::Primary || state.selected != Some(link.id) {
        return;
    }
    let Some(&entity) = state.entities.get(&link.id) else {
        return;
    };
    let Ok((mut transform, grab)) = objects.get_mut(entity) else {
        return;
    };
    let height = transform.translation.y;
    let Some(hit) = ground_hit(&cams, drag.pointer.position, height) else {
        return;
    };
    let grab = grab.map(|g| g.0).unwrap_or(Vec3::ZERO);
    let translation = [hit.x + grab.x, height, hit.z + grab.z];
    state.doc.set_translation(link.id, translation);
    transform.translation.x = translation[0];
    transform.translation.z = translation[2];
}

/// Spawns the demo scene used by headless captures when `EME_EDITOR_DEMO=1`.
pub fn spawn_demo(state: &mut SceneState) {
    if !std::env::var("EME_EDITOR_DEMO").is_ok_and(|v| v == "1") {
        return;
    }
    let first = state.spawn(ObjectKind::Cube);
    state.spawn(ObjectKind::Cube);
    state.spawn(ObjectKind::Sphere);
    state.spawn(ObjectKind::PointLight);
    state.spawn(ObjectKind::Camera);
    state.select(Some(first));
}

/// Drops every authored object; used when the editor screen closes.
pub fn clear_scene(state: &mut SceneState) {
    if state.doc.is_empty() && state.selected.is_none() {
        return;
    }
    state.doc.objects.clear();
    state.selected = None;
    state.touch();
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::FORMAT_VERSION;

    #[test]
    fn slots_never_repeat() {
        let mut seen = Vec::new();
        for slot in 0..64 {
            let pos = cell(slot);
            assert!(!seen.contains(&pos), "slot {slot} reuses {pos:?}");
            seen.push(pos);
        }
    }

    #[test]
    fn cell_spacing_is_one_cell() {
        assert_eq!(cell(0), (-1.0, -1.0));
        assert_eq!(cell(1), (1.0, -1.0));
        assert_eq!(cell(2), (-1.0, 1.0));
        let (ax, _) = cell(0);
        let (bx, _) = cell(1);
        assert_eq!(bx - ax, CELL);
    }

    #[test]
    fn names_count_per_kind() {
        let mut state = SceneState::default();
        assert_eq!(state.next_name(ObjectKind::Cube), "Cube 1");
        assert_eq!(state.next_name(ObjectKind::Cube), "Cube 2");
        assert_eq!(state.next_name(ObjectKind::Sphere), "Sphere 1");
        assert_eq!(state.next_name(ObjectKind::PointLight), "Point Light 1");
    }

    #[test]
    fn primitives_rest_on_the_grid() {
        let (x, z) = cell(0);
        for kind in ObjectKind::ALL {
            let transform = resting(kind, x, z);
            assert_eq!(transform.translation[0], x);
            assert_eq!(transform.translation[2], z);
        }
        assert_eq!(resting(ObjectKind::Cube, x, z).translation[1], CUBE / 2.0);
        assert_eq!(resting(ObjectKind::Empty, x, z).translation[1], 0.0);
        assert!(resting(ObjectKind::Camera, x, z).translation[1] > CUBE);
    }

    #[test]
    fn camera_glyph_is_not_tinted() {
        assert!(!tintable(ObjectKind::Camera));
        assert!(tintable(ObjectKind::Cube));
        assert!(!tintable(ObjectKind::PointLight));
    }

    #[test]
    fn spawn_writes_ids_and_names_into_the_doc() {
        let mut state = SceneState::default();
        let cube = state.spawn(ObjectKind::Cube);
        let sphere = state.spawn(ObjectKind::Sphere);
        assert_ne!(cube, sphere);
        assert_eq!(state.doc.len(), 2);
        assert_eq!(state.doc.get(cube).unwrap().name, "Cube 1");
        assert_eq!(state.doc.get(sphere).unwrap().name, "Sphere 1");
        assert_eq!(state.doc.get(cube).unwrap().kind, ObjectKind::Cube);
        assert_eq!(state.doc.format_version, FORMAT_VERSION);
        assert!(state.entities.is_empty(), "reconcile owns entity spawning");
    }

    #[test]
    fn despawn_clears_selection_and_keeps_the_rest() {
        let mut state = SceneState::default();
        let cube = state.spawn(ObjectKind::Cube);
        let sphere = state.spawn(ObjectKind::Sphere);
        state.select(Some(cube));
        assert!(state.despawn(cube));
        assert_eq!(state.selected, None);
        assert!(state.doc.get(sphere).is_some());
        assert!(!state.despawn(cube));
    }

    #[test]
    fn duplicate_offsets_x_and_selects_nothing() {
        let mut state = SceneState::default();
        let cube = state.spawn(ObjectKind::Cube);
        let x = state.doc.get(cube).unwrap().translation[0];
        let copy = state.duplicate(cube).unwrap();
        assert_eq!(state.doc.get(copy).unwrap().translation[0], x + 1.0);
        assert_eq!(state.selected, None);
        assert_eq!(state.doc.len(), 2);
    }

    #[test]
    fn loaded_doc_keeps_names_unique() {
        let mut state = SceneState::default();
        state.spawn(ObjectKind::Cube);
        state.spawn(ObjectKind::Cube);
        let doc = SceneDoc::from_ron(&state.doc.to_ron().unwrap()).unwrap();

        let mut loaded = SceneState::default();
        loaded.replace_doc(doc);
        assert_eq!(loaded.spawn(ObjectKind::Cube), 3);
        assert_eq!(loaded.doc.get(3).unwrap().name, "Cube 3");
    }

    #[test]
    fn replace_doc_drops_stale_selection() {
        let mut state = SceneState::default();
        state.spawn(ObjectKind::Cube);
        state.select(Some(1));
        let doc = SceneDoc::from_ron(&state.doc.to_ron().unwrap()).unwrap();
        state.replace_doc(doc);
        assert_eq!(state.selected, Some(1));

        state.replace_doc(SceneDoc::new());
        assert_eq!(state.selected, None);
    }
}
