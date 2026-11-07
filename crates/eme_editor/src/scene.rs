//! Viewport scene state: the scene document, the entities that mirror it, and
//! the reconcile/spawn/selection systems shared by the viewport and the docks.

use std::collections::HashMap;

use eme::color::LinearRgba;
use eme::feathers::theme::UiTheme;
use eme::gizmos::transform_gizmo::TransformGizmoState;
use eme::gltf::GltfAssetLabel;
use eme::math::{ops, Rect};
use eme::prelude::*;
use eme::render::render_resource::Face;
use eme::ui::widget::ViewportNode;
use eme::window::{PrimaryWindow, Window};
use eme_editor_core::Selection;
use eme_game_overrides::ModelOverrides;
use eme_game_runtime::{GuidIndex, ModelImport, NodeKey, SceneBehaviour};
use eme_game_scene::{Behaviour, ObjectKind, ObjectTransform, SceneDoc, SceneObject};

use crate::console::ConsoleLog;
use crate::orbit::{FlyMove, ViewMode};
use crate::play::PlayState;
use crate::tokens;
use crate::undo::Undo;

/// Distance a spawned object advances along one grid cell.
pub const CELL: f32 = 2.0;
/// How far the arrow keys nudge the selected object.
pub const MOVE_STEP: f32 = 0.25;
/// Cosine of the shallowest ray angle a ground-plane hit may come from.
const GRAZE: f32 = 0.0872;
/// A ground hit beyond this distance from the camera is a grazing miss.
const MAX_HIT: f32 = 10_000.0;
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

/// Kinds that own a material the editor can recolor. The camera glyph is
/// textured, so tinting its base color would wash the icon out.
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
    /// Selected object ids; `selection.primary` is the one the inspector shows.
    pub selection: Selection<u64>,
    /// Range anchor kept stable across shift clicks.
    pub anchor: Option<u64>,
    /// Bumped on structural changes so UI sync systems can skip unchanged frames.
    pub rev: u64,
    /// Bumped when the document is replaced wholesale, so history can drop.
    epoch: u64,
    next_slot: usize,
    counts: [u32; 9],
}

/// GUID resolution for model instances, built once from the asset root.
#[derive(Resource)]
pub struct ModelIndex(pub GuidIndex);

impl Default for ModelIndex {
    fn default() -> Self {
        Self(GuidIndex::scan(crate::asset_root()))
    }
}

/// Selected node inside an instanced model.
#[derive(Resource, Clone, Debug, Default)]
pub struct NodeSelection {
    /// Scene object that owns the model.
    pub object: Option<u64>,
    /// Node key the inspector shows.
    pub key: Option<NodeKey>,
}

impl NodeSelection {
    /// Clears the node selection.
    pub fn clear(&mut self) {
        self.object = None;
        self.key = None;
    }
}

/// Links a spawned model root back to its scene object id.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModelOwner(pub u64);

/// Asks the spawner to rebuild one model tree, used when a node is included again.
#[derive(Component, Clone, Copy, Default)]
pub struct ReloadModel;

impl SceneState {
    /// Replaces the selection with `target`, or clears it; the anchor follows.
    pub fn select(&mut self, target: Option<u64>) {
        let same = match target {
            Some(id) => self.selection.items.as_slice() == [id],
            None => self.selection.is_empty(),
        };
        self.anchor = target;
        if same {
            return;
        }
        match target {
            Some(id) => self.selection.set(id),
            None => self.selection.clear(),
        }
        self.touch();
    }

    /// Selects exactly `id`.
    pub fn select_only(&mut self, id: u64) {
        self.select(Some(id));
    }

    /// Adds `id` to the selection, or removes it when it was already in.
    pub fn toggle(&mut self, id: u64) {
        self.selection.toggle(id);
        self.anchor = Some(id);
        self.touch();
    }

    /// Selects every object between the anchor and `id`, inclusive.
    ///
    /// The anchor stays where it was, so repeated shift clicks grow the range
    /// from the same origin. Without an anchor the range is just `id`.
    pub fn extend_to(&mut self, id: u64) {
        let Some(to) = self.doc.objects.iter().position(|obj| obj.id == id) else {
            return;
        };
        let from = self
            .anchor
            .and_then(|anchor| self.doc.objects.iter().position(|obj| obj.id == anchor))
            .unwrap_or(to);
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        let ids: Vec<u64> = self.doc.objects[lo..=hi].iter().map(|obj| obj.id).collect();
        self.selection.set_items(ids);
        self.touch();
    }

    /// Selects `ids` in order, with the last as the primary, and anchors on the first.
    pub fn select_many(&mut self, ids: &[u64]) {
        self.selection.set_items(ids.to_vec());
        self.anchor = ids.first().copied();
        self.touch();
    }

    /// Drops the selection and the range anchor.
    pub fn clear_selection(&mut self) {
        if self.selection.is_empty() && self.anchor.is_none() {
            return;
        }
        self.selection.clear();
        self.anchor = None;
        self.touch();
    }

    /// Whether `id` is part of the selection.
    pub fn is_selected(&self, id: u64) -> bool {
        self.selection.contains(&id)
    }

    /// The object the inspector shows, if any is selected.
    pub fn primary(&self) -> Option<u64> {
        self.selection.primary
    }

    /// Every selected id, in selection order.
    pub fn selected_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.selection.iter()
    }

    /// Invalidates the scene UI.
    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// Counter bumped whenever [`Self::replace_doc`] swaps the document.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The tracked object for `id`, if it is a scene object.
    pub fn object(&self, id: u64) -> Option<&SceneObject> {
        self.doc.get(id)
    }

    /// The primary selected object, if any.
    pub fn selected_object(&self) -> Option<&SceneObject> {
        self.selection.primary.and_then(|id| self.doc.get(id))
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
    #[cfg(test)]
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let new_id = self.doc.duplicate(id)?;
        self.touch();
        Some(new_id)
    }

    /// Reparents an object, refusing cycles. Transforms are absolute, so nothing moves.
    pub fn reparent(&mut self, id: u64, parent: Option<u64>) -> bool {
        if !self.doc.set_parent(id, parent) {
            return false;
        }
        self.touch();
        true
    }

    /// Removes an object, dropping it from the selection when it was selected.
    #[cfg(test)]
    pub fn despawn(&mut self, id: u64) -> bool {
        if self.doc.remove(id).is_none() {
            return false;
        }
        self.selection.retain(|it| *it != id);
        if self.anchor == Some(id) {
            self.anchor = self.selection.primary;
        }
        self.touch();
        true
    }

    /// Removes every selected object and clears the selection.
    pub fn despawn_selected(&mut self) -> usize {
        let ids: Vec<u64> = self.selected_ids().collect();
        let mut removed = 0;
        for id in ids {
            if self.doc.remove(id).is_some() {
                removed += 1;
            }
        }
        if removed == 0 {
            return 0;
        }
        self.selection.clear();
        self.anchor = None;
        self.touch();
        removed
    }

    /// Replaces the document, as a scene load does, and resyncs naming state.
    pub fn replace_doc(&mut self, doc: SceneDoc) {
        self.doc = doc;
        let doc = &self.doc;
        self.selection.retain(|id| doc.get(*id).is_some());
        if self.anchor.is_some_and(|id| doc.get(id).is_none()) {
            self.anchor = self.selection.primary;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.resync_counts();
        self.touch();
    }

    /// Swaps in a whole document state, as undo and redo do.
    pub fn restore(&mut self, doc: SceneDoc, selection: Selection<u64>, anchor: Option<u64>) {
        self.doc = doc;
        self.selection = selection;
        self.anchor = anchor;
        self.resync_counts();
        self.touch();
    }

    pub(crate) fn resync_counts(&mut self) {
        let mut counts = [0u32; 9];
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

    /// The next unused name for `kind`, e.g. `Cube 3`.
    pub fn next_name(&mut self, kind: ObjectKind) -> String {
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
        ObjectKind::Empty | ObjectKind::Model => 0.0,
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

/// Marks a scene entity whose document object is gone.
///
/// Despawning it right away races engine systems that queue commands against the
/// entity in the same frame (a point light's frustum `Sphere` is inserted after
/// the editor reconciles), so the actual despawn waits for the end of the frame.
#[derive(Component, Clone, Copy, Default)]
pub struct StaleScene;

/// Removes entities marked [`StaleScene`], after every deferred command has run.
pub fn purge_stale(
    mut commands: Commands,
    stale: Query<Entity, With<StaleScene>>,
    glyphs: Query<(Entity, &GlyphOwner)>,
) {
    for entity in &stale {
        for (glyph, owner) in &glyphs {
            if owner.0 == entity {
                commands.entity(glyph).despawn();
            }
        }
        commands.entity(entity).despawn();
    }
}

/// Brings the entity mirror in line with the document after a `rev` change.
pub fn reconcile_scene(
    mut commands: Commands,
    state: ResMut<SceneState>,
    mut assets: ResMut<SceneAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    index: Res<ModelIndex>,
    theme: Res<UiTheme>,
    play: Res<PlayState>,
    view: Res<ViewMode>,
    links: Query<&SceneLink>,
) {
    let state = state.into_inner();
    let playing = play.running() && *view == ViewMode::Game;
    let ids: Vec<u64> = state.doc.objects.iter().map(|obj| obj.id).collect();

    let stale: Vec<(u64, Entity)> = state
        .entities
        .iter()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale {
        commands.entity(entity).insert(StaleScene);
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
        let spec = obj.behaviour;
        let trs = to_transform(&transform);
        let behaviour = SceneBehaviour { spec, base: trs };

        match state.entities.get(&id).copied() {
            Some(entity) if links.get(entity).is_ok_and(|link| link.kind == kind) => {
                let mut entity = commands.entity(entity);
                entity.insert((Name::new(name), behaviour));
                // the behaviour system owns the transform during an embedded
                // session, so the document must not overwrite it here
                if !playing {
                    entity.insert(trs);
                }
            }
            Some(entity) => {
                commands.entity(entity).insert(StaleScene);
                state.entities.remove(&id);
                let entity = spawn_entity(
                    &mut commands,
                    &mut assets,
                    &mut meshes,
                    &mut materials,
                    &asset_server,
                    &index.0,
                    &theme,
                    id,
                    kind,
                    name,
                    &transform,
                    color,
                    spec,
                    obj.asset.as_ref(),
                    &obj.overrides,
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
                    &index.0,
                    &theme,
                    id,
                    kind,
                    name,
                    &transform,
                    color,
                    spec,
                    obj.asset.as_ref(),
                    &obj.overrides,
                );
                state.entities.insert(id, entity);
            }
        }
    }
}

impl SceneState {
    /// Adds a model object for `rel` (assets-relative), returning its id.
    ///
    /// The sidecar GUID is read (or created) here so the scene stores the durable
    /// identity rather than only a path.
    pub fn spawn_model(&mut self, rel: &str, at: [f32; 3]) -> Option<u64> {
        self.spawn_model_in(&crate::asset_root(), rel, at)
    }

    /// Adds a model object for `rel`, resolved against `root`.
    pub fn spawn_model_in(
        &mut self,
        root: &std::path::Path,
        rel: &str,
        at: [f32; 3],
    ) -> Option<u64> {
        let full = root.join(rel);
        if !full.is_file() {
            warn!("cannot instance {rel}: file is missing");
            return None;
        }
        let sidecar = crate::import::Sidecar::load_or_new(&full);
        let name = full
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Model".into());
        let transform = ObjectTransform::from_translation(at);
        let id = self.doc.add(ObjectKind::Model, name, transform);
        self.doc.set_asset(
            id,
            Some(eme_game_scene::AssetRef {
                guid: sidecar.guid,
                path: rel.to_owned(),
            }),
        );
        self.select_only(id);
        self.touch();
        Some(id)
    }
}

/// One instanced model; the root of a spawned glTF scene.
#[derive(Component, Clone, Default)]
pub struct ModelInstance {
    /// Sidecar GUID of the asset this instance came from.
    pub guid: String,
    /// AssetServer-relative path it was loaded from.
    pub path: String,
}

/// Rebuilds model trees whose overrides need nodes that are currently despawned.
pub fn respawn_model_roots(
    mut commands: Commands,
    mut roots: Query<(Entity, &WorldAssetRoot, &mut ReloadModel)>,
) {
    for (entity, root, _) in &mut roots {
        commands
            .entity(entity)
            .insert(WorldAssetRoot(root.0.clone()));
        commands.entity(entity).remove::<ReloadModel>();
    }
}

/// Mirrors authored overrides from the document onto the live model root.
pub fn sync_model_overrides(
    state: Res<SceneState>,
    mut roots: Query<(&ModelOwner, &mut ModelOverrides)>,
) {
    for (owner, mut live) in &mut roots {
        let Some(obj) = state.doc.get(owner.0) else {
            continue;
        };
        if live.0 != obj.overrides {
            live.0 = obj.overrides.clone();
        }
    }
}

/// Reports orphaned and moved overrides in the console after a spawn or reload.
pub fn report_override_issues(
    roots: Query<(&ModelOwner, &ModelInstance, &eme_game_overrides::ModelOverrideStatus)>,
    mut log: ResMut<ConsoleLog>,
    mut seen: Local<HashMap<u64, u64>>,
) {
    for (owner, instance, status) in &roots {
        if status.revision == 0 || seen.get(&owner.0) == Some(&status.revision) {
            continue;
        }
        seen.insert(owner.0, status.revision);
        for (from, to) in &status.moved {
            log.push(format!(
                "override moved on {}: {} → {}",
                instance.path, from.name, to.name
            ));
        }
        for orphan in &status.orphans {
            let reason = match &orphan.reason {
                eme_game_overrides::OrphanReason::Missing => "no node with this name".to_string(),
                eme_game_overrides::OrphanReason::Renamed { found } => {
                    format!("path now holds \"{found}\"")
                }
            };
            log.push(format!(
                "override orphaned on {}: {} ({reason})",
                instance.path, orphan.key.name
            ));
        }
    }
}

/// Drops a node selection when the primary object changes.
pub fn sync_node_selection(state: Res<SceneState>, mut node: ResMut<NodeSelection>) {
    if node
        .object
        .is_some_and(|object| state.primary() != Some(object))
    {
        node.clear();
    }
}

/// Stand-in shown when a model's asset cannot be found.
#[derive(Component, Clone, Copy, Default)]
pub struct ModelPlaceholder;

/// Resolves an asset reference to an AssetServer-relative path and its pivot settings.
///
/// GUID first, so renaming or moving the file keeps the scene working; the stored
/// path is the fallback for assets imported before the sidecar existed.
fn resolve_model(
    asset: &eme_game_scene::AssetRef,
    index: &GuidIndex,
) -> Option<(String, ModelImport)> {
    let found = index
        .get(&asset.guid)
        .map(std::path::Path::to_path_buf)
        .filter(|path| path.is_file())
        .or_else(|| {
            let candidate = index.root().join(&asset.path);
            candidate.is_file().then_some(candidate)
        })?;
    let rel = found.strip_prefix(index.root()).ok()?;
    Some((
        rel.to_string_lossy().replace('\\', "/"),
        eme_game_runtime::read_model_import(&found),
    ))
}

/// Parent of a model's scene root; carries scale and up-axis conversion only.
#[derive(Component, Clone, Copy, Default)]
pub struct ImportPivot;

/// Carries the change `before` → current on `id` onto every descendant of `id`.
///
/// Document transforms are absolute, so a child follows a parent by moving with the
/// parent's delta: its offset from the parent's old origin is scaled by the change in
/// parent scale, spun by the change in parent rotation, then the parent's translation
/// delta is added. Ids in `skip` are left where they are, for callers that wrote those
/// objects directly in the same frame.
///
/// Non-uniform parent scale combined with a rotated child shears that child; the
/// approximation above ignores the shear rather than giving objects a shear axis.
pub fn propagate(doc: &mut SceneDoc, id: u64, before: ObjectTransform, skip: &[u64]) {
    let Some(now) = doc.get(id).map(SceneObject::transform) else {
        return;
    };
    if before == now {
        return;
    }
    let below = doc.descendants(id);
    if below.is_empty() {
        return;
    }

    let pivot = Vec3::from_array(before.translation);
    let delta = Vec3::from_array(now.translation) - pivot;
    let spin = Quat::from_array(now.rotation) * Quat::from_array(before.rotation).inverse();
    let factor = Vec3::new(
        factor_of(before.scale[0], now.scale[0]),
        factor_of(before.scale[1], now.scale[1]),
        factor_of(before.scale[2], now.scale[2]),
    );

    for child_id in below {
        if skip.contains(&child_id) {
            continue;
        }
        let Some(child) = doc.get_mut(child_id) else {
            continue;
        };
        let offset = Vec3::from_array(child.translation) - pivot;
        child.translation = (pivot + delta + spin * (offset * factor)).to_array();
        child.rotation = (spin * Quat::from_array(child.rotation)).to_array();
        child.scale = (Vec3::from_array(child.scale) * factor).to_array();
    }
}

/// Propagates a frame's batch of direct transform writes onto their descendants.
///
/// A write that already sits below another write in the batch is covered by that
/// ancestor's propagation, so its own subtree is left to the value written directly;
/// walking both would move shared descendants twice.
pub fn propagate_edits(doc: &mut SceneDoc, edits: &[(u64, ObjectTransform)]) {
    let ids: Vec<u64> = edits.iter().map(|(id, _)| *id).collect();
    for (id, before) in edits {
        if ids
            .iter()
            .any(|other| other != id && doc.is_ancestor(*other, *id))
        {
            continue;
        }
        propagate(doc, *id, *before, &ids);
    }
}

/// Scale ratio to carry onto a child, with a collapsed source axis left alone.
fn factor_of(before: f32, now: f32) -> f32 {
    if ops::abs(before) < f32::EPSILON {
        1.0
    } else {
        now / before
    }
}

fn spawn_entity(
    commands: &mut Commands,
    assets: &mut SceneAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    index: &GuidIndex,
    theme: &UiTheme,
    id: u64,
    kind: ObjectKind,
    name: String,
    transform: &ObjectTransform,
    color: Option<[f32; 4]>,
    spec: Behaviour,
    asset: Option<&eme_game_scene::AssetRef>,
    overrides: &[eme_game_scene::NodeOverride],
) -> Entity {
    let trs = to_transform(transform);
    let behaviour = SceneBehaviour { spec, base: trs };
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
        ObjectKind::Model => {
            let resolved = asset.and_then(|asset| resolve_model(asset, index));
            match resolved {
                Some((rel, import)) => {
                    let handle =
                        asset_server.load(GltfAssetLabel::Scene(0).from_asset(rel.clone()));
                    let guid = asset.map(|asset| asset.guid.clone()).unwrap_or_default();
                    let pivot = eme_game_runtime::pivot_transform(import);
                    commands
                        .spawn((trs, Visibility::Inherited))
                        .with_children(|pivot_entity| {
                            pivot_entity
                                .spawn((pivot, ImportPivot, Visibility::Inherited))
                                .with_children(|model| {
                                    model.spawn((
                                        WorldAssetRoot(handle),
                                        ModelInstance { guid, path: rel },
                                        ModelOwner(id),
                                        ModelOverrides(overrides.to_vec()),
                                    ));
                                });
                        })
                        .id()
                }
                None => {
                    warn!(
                        "model asset missing for object {id}: {}",
                        asset.map_or_else(|| "<no reference>".into(), |asset| asset.path.clone())
                    );
                    let material = materials.add(StandardMaterial {
                        base_color: Color::srgb(0.85, 0.15, 0.45),
                        emissive: Color::srgb(0.35, 0.02, 0.15).into(),
                        ..default()
                    });
                    commands
                        .spawn((
                            Mesh3d(assets.cube(meshes)),
                            MeshMaterial3d(material),
                            trs,
                            ModelPlaceholder,
                        ))
                        .id()
                }
            }
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
        .insert((Name::new(name), SceneLink { id, kind }, behaviour))
        .observe(click_object)
        .observe(drag_start)
        .observe(drag_object);
    entity
}

/// Paints each object's authored color, falling back to the theme text color.
pub fn tint_selection(
    state: Res<SceneState>,
    theme: Res<UiTheme>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    let base = theme.color(&tokens::TEXT);

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
        let color = obj
            .color
            .map_or(base, |[r, g, b, a]| Color::linear_rgba(r, g, b, a));
        if material.base_color != color {
            material.base_color = color;
        }
        if material.emissive != LinearRgba::BLACK {
            material.emissive = LinearRgba::BLACK;
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
        let color = if state.is_selected(link.id) {
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

/// Arrow keys nudge every selected object on X/Z; Delete and Backspace remove them.
pub fn keyboard_edit(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut objects: Query<&mut Transform>,
) {
    let ids: Vec<u64> = state.selected_ids().collect();
    if ids.is_empty() {
        return;
    }

    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        undo.checkpoint(&state, "Delete");
        state.despawn_selected();
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

    undo.checkpoint(&state, "Nudge");
    let mut written: Vec<(u64, ObjectTransform)> = Vec::new();
    for id in ids {
        let Some(before) = state.doc.get(id).map(SceneObject::transform) else {
            continue;
        };
        let mut translation = before.translation;
        translation[0] += step.x;
        translation[2] += step.y;
        state.doc.set_translation(id, translation);
        written.push((id, before));
        let Some(&entity) = state.entities.get(&id) else {
            continue;
        };
        if let Ok(mut transform) = objects.get_mut(entity) {
            transform.translation.x = translation[0];
            transform.translation.z = translation[2];
        }
    }
    propagate_edits(&mut state.doc, &written);
    state.touch();
}

/// Starting state of a body drag: the grab point, and every dragged object's
/// translation when the drag began.
#[derive(Component, Clone)]
struct DragGrab {
    plane_y: f32,
    start: Vec3,
    items: Vec<(u64, [f32; 3])>,
}

/// Window pointer position in the viewport panel's pixel space.
fn viewport_pos(
    window: &Window,
    panels: &Query<(&ComputedNode, &UiGlobalTransform), With<ViewportNode>>,
    pos: Vec2,
) -> Option<Vec2> {
    let Ok((node, global)) = panels.single() else {
        return None;
    };
    let rect = Rect::from_center_size(global.translation.trunc(), node.size());
    Some(pos * window.scale_factor() - rect.min)
}

fn ground_hit(
    cams: &Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    position: Vec2,
    height: f32,
) -> Option<Vec3> {
    let (camera, transform) = cams.single().ok()?;
    let ray = camera.viewport_to_world(transform, position).ok()?;
    if ops::abs(ray.direction.dot(Vec3::Y)) < GRAZE {
        return None;
    }
    let hit =
        ray.plane_intersection_point(Vec3::new(0.0, height, 0.0), InfinitePlane3d::new(Vec3::Y))?;
    if hit.distance(ray.origin) > MAX_HIT {
        return None;
    }
    Some(hit)
}

fn click_object(
    click: On<PointerClick>,
    keys: Res<ButtonInput<KeyCode>>,
    links: Query<&SceneLink>,
    fly: Res<FlyMove>,
    mut state: ResMut<SceneState>,
    mut commands: Commands,
) {
    let Ok(link) = links.get(click.entity) else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    match click.button {
        PointerButton::Primary => {
            if shift {
                state.extend_to(link.id);
            } else if ctrl {
                state.toggle(link.id);
            } else {
                state.select_only(link.id);
            }
        }
        PointerButton::Secondary => {
            if fly.suppress_menu || click.duration.as_millis() > 250 {
                return;
            }
            if !state.is_selected(link.id) {
                state.select(Some(link.id));
            }
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
    mut undo: ResMut<Undo>,
    gizmo: Res<TransformGizmoState>,
    links: Query<&SceneLink>,
    cams: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    objects: Query<&Transform>,
    window: Single<&Window, With<PrimaryWindow>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<ViewportNode>>,
    mut commands: Commands,
) {
    let Ok(link) = links.get(start.entity) else {
        return;
    };
    if start.button != PointerButton::Primary {
        return;
    }
    if gizmo.hovered_axis.is_some() || gizmo.active {
        return;
    }
    if !state.is_selected(link.id) {
        return;
    }
    let Some(&entity) = state.entities.get(&link.id) else {
        return;
    };
    let Ok(transform) = objects.get(entity) else {
        return;
    };
    let plane_y = transform.translation.y;
    let Some(pos) = viewport_pos(&window, &panels, start.pointer.position) else {
        return;
    };
    let Some(hit) = ground_hit(&cams, pos, plane_y) else {
        return;
    };

    let items: Vec<(u64, [f32; 3])> = state
        .selected_ids()
        .filter_map(|id| state.doc.get(id).map(|obj| (id, obj.translation)))
        .collect();
    if items.is_empty() {
        return;
    }
    undo.checkpoint(&state, "Move");
    commands.entity(entity).insert(DragGrab {
        plane_y,
        start: hit,
        items,
    });
}

fn drag_object(
    drag: On<PointerDrag>,
    mut state: ResMut<SceneState>,
    gizmo: Res<TransformGizmoState>,
    links: Query<&SceneLink>,
    cams: Query<(&Camera, &GlobalTransform), With<ViewportCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<ViewportNode>>,
    mut objects: Query<(&mut Transform, Option<&DragGrab>)>,
) {
    let Ok(link) = links.get(drag.entity) else {
        return;
    };
    if drag.button != PointerButton::Primary || gizmo.hovered_axis.is_some() || gizmo.active {
        return;
    }
    if !state.is_selected(link.id) {
        return;
    }
    let Some(&entity) = state.entities.get(&link.id) else {
        return;
    };
    let Some(grab) = objects.get(entity).ok().and_then(|(_, grab)| grab.cloned()) else {
        return;
    };
    let Some(pos) = viewport_pos(&window, &panels, drag.pointer.position) else {
        return;
    };
    let Some(hit) = ground_hit(&cams, pos, grab.plane_y) else {
        return;
    };
    let delta = Vec2::new(hit.x - grab.start.x, hit.z - grab.start.z);
    if delta == Vec2::ZERO {
        return;
    }

    let mut written: Vec<(u64, ObjectTransform)> = Vec::new();
    for (id, start) in &grab.items {
        let Some(before) = state.doc.get(*id).map(SceneObject::transform) else {
            continue;
        };
        let translation = [start[0] + delta.x, start[1], start[2] + delta.y];
        state.doc.set_translation(*id, translation);
        written.push((*id, before));
        let Some(&other) = state.entities.get(id) else {
            continue;
        };
        if let Ok((mut transform, _)) = objects.get_mut(other) {
            transform.translation.x = translation[0];
            transform.translation.z = translation[2];
        }
    }
    propagate_edits(&mut state.doc, &written);
}

/// Spawns the demo scene used by headless captures when `EME_EDITOR_DEMO=1`.
pub fn spawn_demo(state: &mut SceneState) {
    if !std::env::var("EME_EDITOR_DEMO").is_ok_and(|v| v == "1") {
        return;
    }
    let first = state.spawn(ObjectKind::Cube);
    state.doc.set_behaviour(
        first,
        Behaviour {
            kind: eme_game_scene::BehaviourKind::Spin,
            ..default()
        },
    );
    state.spawn(ObjectKind::Cube);
    state.spawn(ObjectKind::Sphere);
    state.spawn(ObjectKind::PointLight);
    state.spawn(ObjectKind::Camera);
    state.select_only(first);
}

/// Drops every authored object; used when the editor screen closes.
pub fn clear_scene(state: &mut SceneState) {
    if state.doc.is_empty() && state.selection.is_empty() && state.anchor.is_none() {
        return;
    }
    state.doc.objects.clear();
    state.clear_selection();
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
    fn behaviour_system_does_not_touch_the_document() {
        use core::time::Duration;

        use eme::time::{TimePlugin, TimeUpdateStrategy};
        use eme_game_runtime::run_behaviours;

        let spec = Behaviour {
            kind: eme_game_scene::BehaviourKind::Spin,
            ..default()
        };
        let mut state = SceneState::default();
        let id = state.spawn(ObjectKind::Cube);
        assert!(state.doc.set_behaviour(id, spec));
        let want = state.doc.clone();
        let base = to_transform(&want.get(id).unwrap().transform());

        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                0.5,
            )))
            .insert_resource(state)
            .add_systems(Update, run_behaviours);
        let entity = app
            .world_mut()
            .spawn((SceneBehaviour { spec, base }, base))
            .id();

        app.update();
        let first = *app.world().entity(entity).get::<Transform>().unwrap();
        app.update();
        let second = *app.world().entity(entity).get::<Transform>().unwrap();

        assert_ne!(first.rotation, second.rotation);
        assert_eq!(app.world().resource::<SceneState>().doc, want);
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
        assert_eq!(state.primary(), None);
        assert!(state.selection.is_empty());
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
        assert!(state.selection.is_empty());
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
        assert_eq!(state.primary(), Some(1));

        state.replace_doc(SceneDoc::new());
        assert_eq!(state.primary(), None);
        assert!(state.selection.is_empty());
    }

    fn scene_with(count: usize) -> (SceneState, Vec<u64>) {
        let mut state = SceneState::default();
        let ids = (0..count).map(|_| state.spawn(ObjectKind::Cube)).collect();
        (state, ids)
    }

    #[test]
    fn toggle_adds_and_removes_ids() {
        let (mut state, ids) = scene_with(3);
        state.select_only(ids[0]);
        state.toggle(ids[1]);
        assert_eq!(
            state.selected_ids().collect::<Vec<_>>(),
            vec![ids[0], ids[1]]
        );
        assert_eq!(state.primary(), Some(ids[1]));
        assert_eq!(state.anchor, Some(ids[1]));

        state.toggle(ids[1]);
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![ids[0]]);
        assert_eq!(state.primary(), Some(ids[0]));
    }

    #[test]
    fn extend_selects_an_inclusive_range_from_the_anchor() {
        let (mut state, ids) = scene_with(5);
        state.select_only(ids[1]);
        state.extend_to(ids[3]);
        assert_eq!(
            state.selected_ids().collect::<Vec<_>>(),
            vec![ids[1], ids[2], ids[3]]
        );
        assert_eq!(state.primary(), Some(ids[3]));
        assert_eq!(state.anchor, Some(ids[1]), "extend keeps the anchor");

        state.extend_to(ids[0]);
        assert_eq!(
            state.selected_ids().collect::<Vec<_>>(),
            vec![ids[0], ids[1]]
        );
        assert_eq!(state.anchor, Some(ids[1]));
    }

    #[test]
    fn extend_without_anchor_selects_one() {
        let (mut state, ids) = scene_with(3);
        state.extend_to(ids[2]);
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![ids[2]]);
    }

    #[test]
    fn plain_select_resets_the_anchor() {
        let (mut state, ids) = scene_with(4);
        state.select_only(ids[0]);
        state.extend_to(ids[2]);
        state.select(Some(ids[3]));
        assert_eq!(state.anchor, Some(ids[3]));
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![ids[3]]);

        state.select(None);
        assert_eq!(state.anchor, None);
        assert!(state.selection.is_empty());
    }

    #[test]
    fn despawn_selected_removes_every_id() {
        let (mut state, ids) = scene_with(4);
        state.select_many(&ids[1..3]);
        assert_eq!(state.despawn_selected(), 2);
        assert_eq!(state.doc.len(), 2);
        assert!(state.doc.get(ids[1]).is_none());
        assert!(state.doc.get(ids[2]).is_none());
        assert!(state.selection.is_empty());
        assert_eq!(state.anchor, None);
        assert_eq!(state.despawn_selected(), 0);
    }

    #[test]
    fn replace_doc_retains_live_ids_only() {
        let (mut state, ids) = scene_with(3);
        state.select_many(&ids);
        let mut doc = state.doc.clone();
        doc.remove(ids[0]);
        doc.remove(ids[2]);
        state.replace_doc(doc);
        assert_eq!(state.selected_ids().collect::<Vec<_>>(), vec![ids[1]]);
    }

    #[test]
    fn replace_doc_bumps_the_epoch() {
        let mut state = SceneState::default();
        let before = state.epoch();
        state.replace_doc(SceneDoc::new());
        assert_ne!(state.epoch(), before);
    }

    fn nested(parent_at: [f32; 3], child_at: [f32; 3]) -> (SceneDoc, u64, u64) {
        let mut doc = SceneDoc::new();
        let parent = doc.add(
            ObjectKind::Cube,
            "Parent",
            ObjectTransform::from_translation(parent_at),
        );
        let child = doc.add(
            ObjectKind::Cube,
            "Child",
            ObjectTransform::from_translation(child_at),
        );
        doc.set_parent(child, Some(parent));
        (doc, parent, child)
    }

    fn close(got: [f32; 3], want: [f32; 3]) {
        for i in 0..3 {
            assert!(ops::abs(got[i] - want[i]) < 1e-5, "{got:?} is not {want:?}");
        }
    }

    #[test]
    fn parent_translation_carries_the_child() {
        let (mut doc, parent, child) = nested([0.0; 3], [2.0, 0.0, 0.0]);
        let before = doc.get(parent).unwrap().transform();
        doc.set_translation(parent, [3.0, 0.0, 0.0]);
        propagate(&mut doc, parent, before, &[]);
        close(doc.get(child).unwrap().translation, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn parent_rotation_spins_the_child() {
        let (mut doc, parent, child) = nested([0.0; 3], [2.0, 0.0, 0.0]);
        let before = doc.get(parent).unwrap().transform();
        let spin = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        doc.get_mut(parent).unwrap().rotation = spin.to_array();
        propagate(&mut doc, parent, before, &[]);
        close(doc.get(child).unwrap().translation, [0.0, 0.0, -2.0]);
        assert!(
            Quat::from_array(doc.get(child).unwrap().rotation).abs_diff_eq(spin, 1e-5),
            "child keeps the parent's spin"
        );
    }

    #[test]
    fn parent_scale_grows_the_child() {
        let (mut doc, parent, child) = nested([0.0; 3], [2.0, 0.0, 0.0]);
        let before = doc.get(parent).unwrap().transform();
        doc.get_mut(parent).unwrap().scale = [2.0, 2.0, 2.0];
        propagate(&mut doc, parent, before, &[]);
        close(doc.get(child).unwrap().translation, [4.0, 0.0, 0.0]);
        close(doc.get(child).unwrap().scale, [2.0, 2.0, 2.0]);
    }

    #[test]
    fn propagate_skips_listed_descendants() {
        let (mut doc, parent, child) = nested([0.0; 3], [2.0, 0.0, 0.0]);
        let other = doc.add(
            ObjectKind::Cube,
            "Other",
            ObjectTransform::from_translation([2.0, 1.0, 0.0]),
        );
        doc.set_parent(other, Some(parent));
        let before = doc.get(parent).unwrap().transform();
        doc.set_translation(parent, [3.0, 0.0, 0.0]);
        propagate(&mut doc, parent, before, &[child]);
        close(doc.get(child).unwrap().translation, [2.0, 0.0, 0.0]);
        close(doc.get(other).unwrap().translation, [5.0, 1.0, 0.0]);
    }

    #[test]
    fn propagate_without_descendants_is_a_no_op() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Lonely",
            ObjectTransform::from_translation([1.0, 2.0, 3.0]),
        );
        let before = doc.get(id).unwrap().transform();
        doc.set_translation(id, [9.0, 2.0, 3.0]);
        let want = doc.clone();
        propagate(&mut doc, id, before, &[]);
        assert_eq!(doc, want);
    }

    #[test]
    fn a_child_moving_leaves_its_parent_alone() {
        let (mut doc, parent, child) = nested([1.0, 0.0, 0.0], [3.0, 0.0, 0.0]);
        let leaf = doc.add(
            ObjectKind::Cube,
            "Leaf",
            ObjectTransform::from_translation([4.0, 0.0, 0.0]),
        );
        doc.set_parent(leaf, Some(child));
        let before = doc.get(child).unwrap().transform();
        doc.set_translation(child, [3.0, 0.0, 2.0]);
        propagate(&mut doc, child, before, &[]);
        close(doc.get(parent).unwrap().translation, [1.0, 0.0, 0.0]);
        close(doc.get(leaf).unwrap().translation, [4.0, 0.0, 2.0]);
    }

    #[test]
    fn a_batch_moves_shared_descendants_once() {
        let (mut doc, parent, child) = nested([0.0; 3], [2.0, 0.0, 0.0]);
        let leaf = doc.add(
            ObjectKind::Cube,
            "Leaf",
            ObjectTransform::from_translation([4.0, 0.0, 0.0]),
        );
        doc.set_parent(leaf, Some(child));
        let parent_before = doc.get(parent).unwrap().transform();
        let child_before = doc.get(child).unwrap().transform();
        doc.set_translation(parent, [3.0, 0.0, 0.0]);
        doc.set_translation(child, [5.0, 0.0, 0.0]);
        propagate_edits(&mut doc, &[(parent, parent_before), (child, child_before)]);
        close(doc.get(leaf).unwrap().translation, [7.0, 0.0, 0.0]);
    }

    #[test]
    fn moving_a_parent_keeps_the_selection_and_anchor() {
        let mut state = SceneState::default();
        let parent = state.spawn(ObjectKind::Cube);
        let child = state.spawn(ObjectKind::Cube);
        assert!(state.reparent(child, Some(parent)));
        state.select_many(&[parent, child]);
        let before = state.doc.get(parent).unwrap().transform();
        state.doc.set_translation(parent, [4.0, 0.0, 0.0]);
        propagate(&mut state.doc, parent, before, &[]);
        assert_eq!(
            state.selected_ids().collect::<Vec<_>>(),
            vec![parent, child]
        );
        assert_eq!(state.primary(), Some(child));
        assert_eq!(state.anchor, Some(parent));
    }

    #[test]
    fn a_sidecar_scale_change_leaves_the_authored_transform_alone() {
        let dir = std::env::temp_dir().join(format!("fabr-pivot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("chair.glb");
        std::fs::write(&model, b"glTF").unwrap();
        let sidecar = model.with_file_name("chair.glb.import.ron");
        std::fs::write(
            &sidecar,
            "(version: 1, guid: \"chair-guid\", settings: (transform: (scale: 1.0, up_axis: Y)))",
        )
        .unwrap();

        let mut state = SceneState::default();
        let id = state
            .spawn_model_in(&dir, "chair.glb", [2.5, 1.0, -4.0])
            .unwrap();
        let authored = state.doc.get(id).unwrap().transform();
        assert_eq!(authored.translation, [2.5, 1.0, -4.0]);
        assert_eq!(authored.scale, [1.0, 1.0, 1.0]);
        assert_eq!(authored.rotation, [0.0, 0.0, 0.0, 1.0]);

        std::fs::write(
            &sidecar,
            "(version: 1, guid: \"chair-guid\", settings: (transform: (scale: 3.0, up_axis: Z)))",
        )
        .unwrap();
        let asset = state.doc.get(id).unwrap().asset.clone().unwrap();
        let index = GuidIndex::scan(&dir);
        let (_, import) = resolve_model(&asset, &index).unwrap();
        let pivot = eme_game_runtime::pivot_transform(import);
        assert_eq!(pivot.scale, Vec3::splat(3.0));

        let after = state.doc.get(id).unwrap().transform();
        assert_eq!(after.translation, authored.translation);
        assert_eq!(after.scale, authored.scale);
        assert_eq!(after.rotation, authored.rotation);
    }

    #[test]
    fn orphan_warnings_reach_the_console() {
        use eme::ecs::system::RunSystemOnce;
        use eme_game_overrides::{ModelOverrideStatus, OrphanOverride, OrphanReason};

        let mut world = World::new();
        world.insert_resource(ConsoleLog::default());
        world.spawn((
            ModelOwner(1),
            ModelInstance::default(),
            ModelOverrideStatus {
                revision: 1,
                orphans: vec![OrphanOverride {
                    key: NodeKey {
                        path: vec![0],
                        name: "Old".into(),
                    },
                    reason: OrphanReason::Renamed {
                        found: "New".into(),
                    },
                }],
                ..Default::default()
            },
        ));
        world.run_system_once(report_override_issues).unwrap();

        let log = world.resource::<ConsoleLog>();
        assert!(
            log.lines()
                .iter()
                .any(|line| line.contains("orphaned") && line.contains("Old") && line.contains("New")),
            "{:?}",
            log.lines()
        );
    }

    #[test]
    fn the_importer_and_the_runtime_agree_on_node_keys() {
        use eme::ecs::system::RunSystemOnce;

        let dir = std::env::temp_dir().join(format!("fabr-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dupes.gltf");
        std::fs::write(
            &path,
            r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
                "nodes":[{"name":"Node","children":[1]},{"name":"Node"}]}"#,
        )
        .unwrap();
        let report =
            crate::import::import(&path, &crate::import::ImportSettings::default()).unwrap();
        let mut imported: Vec<NodeKey> = report.nodes.iter().map(|node| node.key.clone()).collect();
        imported.sort();

        let mut world = World::new();
        let root = world.spawn(ModelOverrides::default()).id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let first = world.spawn(Name::new("Node")).id();
        let second = world.spawn(Name::new("Node")).id();
        world.entity_mut(first).add_children(&[second]);
        world.entity_mut(scene).add_children(&[first]);
        world.entity_mut(root).add_children(&[scene]);
        world.run_system_once(eme_game_overrides::sync_model_nodes).unwrap();

        let catalog = world
            .get::<eme_game_overrides::ModelNodeCatalog>(root)
            .unwrap();
        let mut tagged: Vec<NodeKey> = catalog.nodes.iter().map(|node| node.key.clone()).collect();
        tagged.sort();
        assert_eq!(tagged, imported);
        assert_eq!(imported.len(), 2);
    }
}
