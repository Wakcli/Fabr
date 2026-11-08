use core::fmt;
use std::path::{Path, PathBuf};

use eme::prelude::*;
use eme::render::view::Msaa;
use eme_game_overrides::ModelOverrides;
use eme_game_runtime::{GuidIndex, ModelImport, SceneBehaviour};
use eme_anim::AnimationSource;
use eme_game_runtime::{KinematicController, PhysicsBody};
use eme_net::{NetKind, ReplicatedObject, ReplicatedSpawn};
use fabr::ScriptedObject;
use eme_game_scene::{
    Behaviour, BehaviourKind, ObjectKind, ObjectTransform, SceneDoc, SceneObject,
};

const SCENE_IN_CWD: &str = "scenes/main.ember";
const SCENE_NEXT_TO_EXE: &str = "scene.ember";

const CUBE: f32 = 1.0;
const SPHERE_R: f32 = 0.5;
const CYL_R: f32 = 0.5;
const CYL_H: f32 = 1.0;
const PLANE: f32 = 2.0;

const DEFAULT_CAM: Vec3 = Vec3::new(7.0, 5.0, 7.0);
const NEUTRAL: Color = Color::srgb(0.62, 0.62, 0.66);
const SUN_ILLUMINANCE: f32 = 8_000.0;
const POINT_INTENSITY: f32 = 800_000.0;
const POINT_RANGE: f32 = 25.0;
const AMBIENT: f32 = 200.0;
const ROUGHNESS: f32 = 0.55;

/// Marks an entity spawned from the current scene document.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SceneRoot;

#[derive(Resource, Debug)]
pub struct LoadedScene {
    pub doc: SceneDoc,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct LoadError {
    tried: Vec<(PathBuf, String)>,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no scene loaded")?;
        for (path, err) in &self.tried {
            write!(f, "\n  {}: {err}", path.display())?;
        }
        Ok(())
    }
}

pub fn load(explicit: Option<&Path>) -> Result<LoadedScene, LoadError> {
    let mut tried = Vec::new();
    for path in candidates(explicit) {
        match read(&path) {
            Ok(doc) => return Ok(LoadedScene { doc, path }),
            Err(err) => tried.push((path, err)),
        }
    }
    Err(LoadError { tried })
}

fn read(path: &Path) -> Result<SceneDoc, String> {
    let src = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    SceneDoc::from_ron(&src).map_err(|err| err.to_string())
}

fn candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = explicit {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        out.push(dir.join(SCENE_NEXT_TO_EXE));
    }
    out.push(PathBuf::from(SCENE_IN_CWD));
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Body {
    Empty,
    Shape(Shape),
    PointLight,
    Sun,
    Camera,
    Model,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    Cube,
    Sphere,
    Cylinder,
    Plane,
}

/// Resolves a model reference the same way the editor does: GUID, then path.
fn model_path(
    asset: &eme_game_scene::AssetRef,
    index: &GuidIndex,
) -> Option<(String, ModelImport)> {
    let found = index
        .get(&asset.guid)
        .map(Path::to_path_buf)
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

fn body(kind: ObjectKind) -> Body {
    match kind {
        ObjectKind::Empty => Body::Empty,
        ObjectKind::Cube => Body::Shape(Shape::Cube),
        ObjectKind::Sphere => Body::Shape(Shape::Sphere),
        ObjectKind::Cylinder => Body::Shape(Shape::Cylinder),
        ObjectKind::Plane => Body::Shape(Shape::Plane),
        ObjectKind::PointLight => Body::PointLight,
        ObjectKind::Sun => Body::Sun,
        ObjectKind::Camera => Body::Camera,
        ObjectKind::Model => Body::Model,
    }
}

impl Shape {
    fn index(self) -> usize {
        self as usize
    }

    fn mesh(self) -> Mesh {
        match self {
            Shape::Cube => Cuboid::new(CUBE, CUBE, CUBE).into(),
            Shape::Sphere => Sphere::new(SPHERE_R).mesh().ico(4).unwrap(),
            Shape::Cylinder => Cylinder::new(CYL_R, CYL_H).into(),
            Shape::Plane => Plane3d::default().mesh().size(PLANE, PLANE).into(),
        }
    }
}

fn to_transform(translation: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) -> Transform {
    Transform::from_translation(Vec3::from_array(translation))
        .with_rotation(Quat::from_array(rotation))
        .with_scale(Vec3::from_array(scale))
}

fn behaviour_for(obj: &SceneObject) -> SceneBehaviour {
    SceneBehaviour {
        spec: obj.behaviour,
        base: to_transform(obj.translation, obj.rotation, obj.scale),
    }
}

pub fn announce(scene: Res<LoadedScene>) {
    if scene.doc.objects.is_empty() {
        warn!(
            "scene has no objects: {} will render an empty world",
            scene.path.display()
        );
    }
    info!(
        "loaded {} objects from {}",
        scene.doc.len(),
        scene.path.display()
    );
}

fn sun_shadows(quality: bool, has_sun: bool) -> bool {
    quality && !has_sun
}

/// MSAA sample count from project settings.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlayerMsaa(pub u32);

/// Whether authored suns cast shadows, from project settings.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PlayerShadows(pub bool);

/// Loads a requested scene, replacing the current document.
///
/// Paths resolve against the working directory first, then against the current
/// scene's folder. A failed load leaves the running scene untouched.
pub fn handle_scene_request(
    mut requests: ResMut<fabr::SceneRequests>,
    mut scene: ResMut<LoadedScene>,
) {
    if !requests.pending() {
        return;
    }
    let reload = requests.reload;
    let requested = requests.load.take();
    requests.reload = false;

    let target = if reload {
        scene.path.clone()
    } else {
        let Some(requested) = requested else {
            return;
        };
        resolve_scene(&requested, &scene.path)
    };
    match read(&target) {
        Ok(doc) => {
            info!(
                "scene: loaded {} objects from {}",
                doc.len(),
                target.display()
            );
            scene.doc = doc;
            scene.path = target;
        }
        Err(err) => warn!("scene: could not load {}: {err}", target.display()),
    }
}

fn resolve_scene(requested: &str, current: &Path) -> PathBuf {
    let path = PathBuf::from(requested);
    if path.is_file() {
        return path;
    }
    let beside = current
        .parent()
        .map(|dir| dir.join(&path))
        .unwrap_or_else(|| path.clone());
    if beside.is_file() {
        return beside;
    }
    path
}

pub fn spawn_scene(
    mut commands: Commands,
    msaa: Res<PlayerMsaa>,
    shadow_quality: Res<PlayerShadows>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    scene: Res<LoadedScene>,
    roots: Query<Entity, With<SceneRoot>>,
) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    let index = GuidIndex::scan(crate::asset_root());
    let mut view: Option<Transform> = None;
    let mut cache: [Option<Handle<Mesh>>; 4] = [const { None }; 4];
    // the editor viewport owns a shadow-casting sun; the player has none, so the
    // first authored sun takes that job
    let mut has_sun = false;

    for obj in &scene.doc.objects {
        let trs = to_transform(obj.translation, obj.rotation, obj.scale);
        let behaviour = behaviour_for(obj);
        let name = Name::new(obj.name.clone());
        let script = obj.script.clone().map(|script| ScriptedObject {
            object_id: obj.id,
            script,
        });
        let physics = obj
            .physics
            .as_ref()
            .filter(|spec| !spec.controller)
            .map(PhysicsBody::from_spec);
        let controller = obj
            .physics
            .as_ref()
            .filter(|spec| spec.controller)
            .map(|spec| KinematicController::new(Vec3::from_array(spec.half_extents)));
        let light = obj.light.unwrap_or_default();
        let light_strength = light.intensity.clamp(0.0, 8.0);
        let light_color =
            Color::linear_rgb(light.color[0], light.color[1], light.color[2]);
        match body(obj.kind) {
            Body::Empty => {
                let mut spawned = commands.spawn((
                    trs,
                    behaviour,
                    name,
                    ReplicatedObject(obj.id),
                    SceneRoot,
                ));
                if let Some(script) = script {
                    spawned.insert(script);
                }
                if let Some(physics) = physics {
                    spawned.insert(physics);
                }
                if let Some(controller) = controller {
                    spawned.insert(controller);
                }
            }
            Body::Camera => {
                view.get_or_insert(trs);
            }
            Body::Model => {
                let Some(asset) = obj.asset.as_ref() else {
                    warn!("model object {} has no asset reference", obj.name);
                    continue;
                };
                let resolved = model_path(asset, &index);
                match resolved {
                    Some((rel, import)) => {
                        let handle =
                            asset_server.load(GltfAssetLabel::Scene(0).from_asset(rel.clone()));
                        let pivot = eme_game_runtime::pivot_transform(import);
                        let mut spawned = commands.spawn((
                            trs,
                            behaviour,
                            name,
                            Visibility::Inherited,
                            ReplicatedObject(obj.id),
                            SceneRoot,
                        ));
                        if let Some(script) = script {
                            spawned.insert(script);
                        }
                        if let Some(physics) = physics {
                            spawned.insert(physics);
                        }
                        spawned.insert(AnimationSource {
                            gltf: asset_server.load(rel.clone()),
                            selected: obj.animation.as_ref().and_then(|animation| animation.clip.clone()),
                            autoplay: obj.animation.as_ref().is_some_and(|animation| animation.autoplay),
                            speed: obj.animation.as_ref().map_or(1.0, |animation| animation.speed),
                            looping: obj.animation.as_ref().is_none_or(|animation| animation.looping),
                        });
                        spawned
                            .with_children(|parent| {
                                parent
                                    .spawn((pivot, Visibility::Inherited))
                                    .with_children(|pivot| {
                                        pivot.spawn((
                                            WorldAssetRoot(handle),
                                            ModelOverrides(obj.overrides.clone()),
                                        ));
                                    });
                            });
                    }
                    None => warn!(
                        "model asset missing for {}: {}",
                        obj.name, asset.path
                    ),
                }
            }
            Body::PointLight => {
                let mut spawned = commands.spawn((
                    PointLight {
                        intensity: POINT_INTENSITY * light_strength,
                        range: POINT_RANGE,
                        color: light_color,
                        ..default()
                    },
                    trs,
                    behaviour,
                    name,
                    ReplicatedObject(obj.id),
                    SceneRoot,
                ));
                if let Some(script) = script {
                    spawned.insert(script);
                }
                if let Some(physics) = physics {
                    spawned.insert(physics);
                }
                if let Some(controller) = controller {
                    spawned.insert(controller);
                }
            }
            Body::Sun => {
                let mut spawned = commands.spawn((
                    DirectionalLight {
                        illuminance: SUN_ILLUMINANCE * light_strength,
                        color: light_color,
                        shadow_maps_enabled: sun_shadows(shadow_quality.0, has_sun),
                        ..default()
                    },
                    trs,
                    behaviour,
                    name,
                    ReplicatedObject(obj.id),
                    SceneRoot,
                ));
                if let Some(script) = script {
                    spawned.insert(script);
                }
                if let Some(physics) = physics {
                    spawned.insert(physics);
                }
                if let Some(controller) = controller {
                    spawned.insert(controller);
                }
                has_sun = true;
            }
            Body::Shape(shape) => {
                let mesh = cache[shape.index()]
                    .get_or_insert_with(|| meshes.add(shape.mesh()))
                    .clone();
                let base = obj
                    .color
                    .map_or(NEUTRAL, |[r, g, b, a]| Color::linear_rgba(r, g, b, a));
                let mut material = StandardMaterial::from(base);
                material.perceptual_roughness = ROUGHNESS;
                material.metallic = 0.0;
                material.alpha_mode = if obj.color.is_some_and(|color| color[3] < 1.0) {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                };
                let kind = match shape {
                    Shape::Cube => NetKind::Cube,
                    Shape::Sphere => NetKind::Sphere,
                    Shape::Cylinder => NetKind::Cylinder,
                    Shape::Plane => NetKind::Plane,
                };
                let net_color = obj.color.unwrap_or([0.62, 0.62, 0.66, 1.0]);
                let mut spawned = commands.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(materials.add(material)),
                    trs,
                    behaviour,
                    name,
                    ReplicatedObject(obj.id),
                    ReplicatedSpawn {
                        kind,
                        color: net_color,
                    },
                    SceneRoot,
                ));
                if let Some(script) = script {
                    spawned.insert(script);
                }
                if let Some(physics) = physics {
                    spawned.insert(physics);
                }
                if let Some(controller) = controller {
                    spawned.insert(controller);
                }
            }
        }
    }

    if !has_sun {
        commands.spawn((
            DirectionalLight {
                illuminance: SUN_ILLUMINANCE,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }

    let view = view.unwrap_or_else(|| {
        Transform::from_translation(DEFAULT_CAM).looking_at(Vec3::ZERO, Vec3::Y)
    });
    commands.spawn((Camera3d::default(), view, Msaa::from_samples(msaa.0), SceneRoot));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.80, 0.85, 1.0),
        brightness: AMBIENT,
        ..default()
    });
}

pub fn demo_doc() -> SceneDoc {
    let mut doc = SceneDoc::new();
    let cube = doc.add(
        ObjectKind::Cube,
        "Cube",
        ObjectTransform::from_translation([-1.5, 0.5, 0.0]),
    );
    doc.set_behaviour(
        cube,
        Behaviour {
            kind: BehaviourKind::Spin,
            ..default()
        },
    );
    doc.add(
        ObjectKind::Sphere,
        "Sphere",
        ObjectTransform::from_translation([0.0, 0.5, 0.0]),
    );
    let cylinder = doc.add(
        ObjectKind::Cylinder,
        "Cylinder",
        ObjectTransform::from_translation([1.5, 0.5, 0.0]),
    );
    doc.get_mut(cylinder).unwrap().color = Some([0.20, 0.45, 0.55, 1.0]);
    doc.add(
        ObjectKind::Plane,
        "Ground",
        ObjectTransform::from_translation([0.0, 0.0, 0.0]).with_scale([4.0, 1.0, 4.0]),
    );
    doc.add(
        ObjectKind::Sun,
        "Sun",
        ObjectTransform::from_translation([0.0, 4.0, 0.0])
            .with_rotation((Quat::from_rotation_x(-0.9) * Quat::from_rotation_y(0.6)).to_array()),
    );
    doc.add(
        ObjectKind::Camera,
        "Camera",
        ObjectTransform::from_translation(DEFAULT_CAM.to_array())
            .with_rotation(look_at(DEFAULT_CAM).to_array()),
    );
    doc
}

fn look_at(eye: Vec3) -> Quat {
    Transform::from_translation(eye)
        .looking_at(Vec3::ZERO, Vec3::Y)
        .rotation
}

/// Builds visuals for entities spawned by replication.
pub fn materialize_replicated(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spawned: Query<(Entity, &ReplicatedSpawn), Without<Mesh3d>>,
) {
    for (entity, spawn) in &spawned {
        let mesh = match spawn.kind {
            NetKind::Cube => Cuboid::new(CUBE, CUBE, CUBE).into(),
            NetKind::Sphere => Sphere::new(SPHERE_R).mesh().ico(4).unwrap(),
            NetKind::Cylinder => Cylinder::new(CYL_R, CYL_H).into(),
            NetKind::Plane => Plane3d::default().mesh().size(PLANE, PLANE).into(),
        };
        let color = Color::linear_rgba(
            spawn.color[0],
            spawn.color[1],
            spawn.color[2],
            spawn.color[3],
        );
        let mut material = StandardMaterial::from(color);
        material.perceptual_roughness = ROUGHNESS;
        material.metallic = 0.0;
        commands.entity(entity).insert((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    use eme::time::{TimePlugin, TimeUpdateStrategy};
    use eme_game_runtime::run_behaviours;
    use eme_game_scene::FORMAT_VERSION;

    #[test]
    fn player_applies_the_shared_override_layer() {
        use eme::ecs::system::RunSystemOnce;
        use eme_game_overrides::ModelNodeCatalog;
        use eme_game_scene::{NodeKey, NodeOverride, OverrideOp};

        let mut world = World::new();
        let root = world
            .spawn(ModelOverrides(vec![NodeOverride {
                key: NodeKey {
                    path: vec![0],
                    name: "Seat".into(),
                },
                op: OverrideOp::SetTransform(ObjectTransform::from_translation([4.0, 0.0, 0.0])),
            }]))
            .id();
        let scene = world.spawn(Name::new("Scene0")).id();
        let seat = world.spawn(Name::new("Seat")).id();
        world.entity_mut(scene).add_children(&[seat]);
        world.entity_mut(root).add_children(&[scene]);
        world
            .run_system_once(eme_game_overrides::sync_model_nodes)
            .unwrap();

        assert_eq!(
            world.get::<Transform>(seat).unwrap().translation,
            Vec3::new(4.0, 0.0, 0.0)
        );
        let catalog = world.get::<ModelNodeCatalog>(root).unwrap();
        assert_eq!(catalog.nodes[0].key.name, "Seat");
    }

    #[test]
    fn quality_shadows_only_apply_to_the_first_sun() {
        assert!(sun_shadows(true, false));
        assert!(!sun_shadows(true, true));
        assert!(!sun_shadows(false, false));
    }

    #[test]
    fn every_kind_has_a_body() {
        let expected = [
            (ObjectKind::Empty, Body::Empty),
            (ObjectKind::Cube, Body::Shape(Shape::Cube)),
            (ObjectKind::Sphere, Body::Shape(Shape::Sphere)),
            (ObjectKind::Cylinder, Body::Shape(Shape::Cylinder)),
            (ObjectKind::Plane, Body::Shape(Shape::Plane)),
            (ObjectKind::PointLight, Body::PointLight),
            (ObjectKind::Sun, Body::Sun),
            (ObjectKind::Camera, Body::Camera),
            (ObjectKind::Model, Body::Model),
        ];
        for (kind, want) in expected {
            assert_eq!(body(kind), want, "{}", kind.label());
        }
        assert_eq!(expected.len(), ObjectKind::ALL.len());
    }

    #[test]
    fn demo_scene_round_trips_through_ron() {
        let doc = demo_doc();
        let text = doc.to_ron().unwrap();
        let back = SceneDoc::from_ron(&text).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.format_version, FORMAT_VERSION);
        for kind in [
            ObjectKind::Cube,
            ObjectKind::Sphere,
            ObjectKind::Cylinder,
            ObjectKind::Plane,
            ObjectKind::Sun,
            ObjectKind::Camera,
        ] {
            assert!(
                back.objects.iter().any(|obj| obj.kind == kind),
                "{} missing from the demo scene",
                kind.label()
            );
        }
        assert!(
            back.objects
                .iter()
                .any(|obj| obj.behaviour.kind == BehaviourKind::Spin),
            "the demo scene should show motion"
        );
    }

    #[test]
    fn a_spawned_spin_advances_between_updates() {
        let doc = demo_doc();
        let obj = doc
            .iter()
            .find(|obj| obj.behaviour.kind == BehaviourKind::Spin)
            .expect("demo scene has a spinning object");

        let mut app = App::new();
        app.add_plugins(TimePlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                0.5,
            )))
            .add_systems(Update, run_behaviours);

        let entity = app
            .world_mut()
            .spawn((
                behaviour_for(obj),
                to_transform(obj.translation, obj.rotation, obj.scale),
            ))
            .id();

        app.update();
        let first = *app.world().entity(entity).get::<Transform>().unwrap();
        app.update();
        let second = *app.world().entity(entity).get::<Transform>().unwrap();

        assert_ne!(first.rotation, second.rotation);
        assert_ne!(first, second);
    }

    #[test]
    fn demo_camera_looks_at_the_origin() {
        let rotation = look_at(DEFAULT_CAM);
        let forward = rotation * Vec3::NEG_Z;
        assert!(forward.dot(-DEFAULT_CAM.normalize()) > 0.99);
    }

    #[test]
    fn explicit_path_replaces_the_default_chain() {
        let explicit = Path::new("build/game.ember");
        assert_eq!(candidates(Some(explicit)), vec![explicit.to_path_buf()]);
    }

    #[test]
    fn defaults_end_in_cwd_and_try_next_to_the_exe_first() {
        let default = candidates(None);
        assert_eq!(default.last().unwrap(), Path::new(SCENE_IN_CWD));
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            default.first().unwrap(),
            &exe.parent().unwrap().join(SCENE_NEXT_TO_EXE)
        );
    }

    #[test]
    fn missing_scene_names_every_attempt() {
        let err = load(Some(Path::new("definitely/missing.ember"))).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("definitely/missing.ember"), "{text}");
        assert!(text.contains("no scene loaded"), "{text}");
    }

    #[test]
    fn load_reads_an_explicit_scene() {
        let path = std::env::temp_dir().join(format!("eme_player_{}.ember", std::process::id()));
        std::fs::write(&path, demo_doc().to_ron().unwrap()).unwrap();
        let loaded = load(Some(&path)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.path, path);
        assert_eq!(loaded.doc, demo_doc());
    }

    #[test]
    fn malformed_scene_reports_the_parser_error() {
        let path =
            std::env::temp_dir().join(format!("eme_player_bad_{}.ember", std::process::id()));
        std::fs::write(&path, "not a scene").unwrap();
        let err = load(Some(&path)).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert!(err.to_string().contains("could not decode scene"), "{err}");
    }

    fn scene_with(count: usize) -> SceneDoc {
        let mut doc = SceneDoc::new();
        for i in 0..count {
            doc.add(
                ObjectKind::Cube,
                format!("Cube {i}"),
                ObjectTransform::default(),
            );
        }
        doc
    }

    fn request_world(load: Option<&Path>, reload: bool, current: &Path) -> World {
        let mut world = World::new();
        world.insert_resource(fabr::SceneRequests {
            load: load.map(|path| path.to_string_lossy().into_owned()),
            reload,
        });
        world.insert_resource(LoadedScene {
            doc: scene_with(1),
            path: current.to_path_buf(),
        });
        world
    }

    #[test]
    fn a_scene_request_replaces_the_document() {
        use eme::ecs::system::RunSystemOnce;

        let dir = std::env::temp_dir().join(format!("eme_player_req_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let next = dir.join("next.ember");
        std::fs::write(&next, scene_with(3).to_ron().unwrap()).unwrap();
        let current = dir.join("current.ember");
        let mut world = request_world(Some(&next), false, &current);
        world.run_system_once(handle_scene_request).unwrap();
        let scene = world.resource::<LoadedScene>();
        assert_eq!(scene.doc.len(), 3);
        assert_eq!(scene.path, next);
        assert!(!world.resource::<fabr::SceneRequests>().pending());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_scene_keeps_the_current_document() {
        use eme::ecs::system::RunSystemOnce;

        let dir = std::env::temp_dir().join(format!("eme_player_miss_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let current = dir.join("current.ember");
        let mut world = request_world(Some(Path::new("/does/not/exist.ember")), false, &current);
        world.run_system_once(handle_scene_request).unwrap();
        let scene = world.resource::<LoadedScene>();
        assert_eq!(scene.doc.len(), 1);
        assert_eq!(scene.path, current);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reload_rereads_the_current_path() {
        use eme::ecs::system::RunSystemOnce;

        let dir = std::env::temp_dir().join(format!("eme_player_reload_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("level.ember");
        std::fs::write(&path, scene_with(1).to_ron().unwrap()).unwrap();
        let mut world = request_world(None, true, &path);
        world.run_system_once(handle_scene_request).unwrap();
        assert_eq!(world.resource::<LoadedScene>().doc.len(), 1);

        std::fs::write(&path, scene_with(2).to_ron().unwrap()).unwrap();
        world.resource_mut::<fabr::SceneRequests>().reload = true;
        world.run_system_once(handle_scene_request).unwrap();
        assert_eq!(world.resource::<LoadedScene>().doc.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
