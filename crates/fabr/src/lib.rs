#![expect(unsafe_code, reason = "the gameplay entry point casts the host App pointer")]

//! Public Fabr game SDK.
//!
//! Game code should depend on this crate rather than the engine internals. It
//! exposes the stable authoring/runtime surface: scene documents, Lua scripts,
//! input actions and project settings. The editor and player use the same
//! underlying crates; this facade keeps their names and versions in one place.

pub use eme::editor;
pub use eme::prelude::App;

pub use eme_editor_project::{InputSettings, PlayerSettings, ProjectSettings, SETTINGS_FILE};
pub use eme_game_scene::{
    AssetRef, Behaviour, BehaviourKind, NodeKey, NodeOverride, ObjectKind, ObjectTransform,
    OverrideOp, SceneDoc, SceneError, SceneObject, ScriptLanguage, ScriptRef, ScriptValue,
    TransformDelta, FORMAT_VERSION,
};
pub use eme_script::{
    key_binding, script_files, script_property_defaults, update_scripts, AudioBus, AudioMix,
    InputActions, SceneRequests, ScriptLog, ScriptPlugin, ScriptReload, ScriptRoot, SceneOp, ScriptUi,
    ScriptedObject, ScriptsActive, UiElement,
};

use eme::prelude::*;
use std::path::Path;

/// Plugin ABI the generated game entry points speak.
pub const PLUGIN_ABI_VERSION: u32 = eme_plugin::ABI_VERSION;

/// A Rust game: the typed gameplay entry point of the SDK.
///
/// The lifecycle mirrors the Lua (`on_start`/`on_update`/`on_stop`) and C#
/// (`OnStart`/`OnUpdate`/`OnStop`) contracts, so the same game logic can move
/// between the three languages without changing shape. `build` installs extra
/// systems and resources; the lifecycle hooks get a [`GameContext`] for
/// value-based world access.
pub trait FabrGame: Send + Sync + 'static {
    /// Installs the game's systems, resources and plugins.
    fn build(&self, _app: &mut App) {}

    /// Runs once after the host app starts.
    fn on_start(&mut self, _ctx: &mut GameContext) {}

    /// Runs every frame with the frame's delta time in seconds.
    fn on_update(&mut self, _ctx: &mut GameContext, _dt: f32) {}

    /// Runs once when the host app is about to exit.
    fn on_stop(&mut self, _ctx: &mut GameContext) {}
}

/// Value-based world access handed to a game's lifecycle hooks.
///
/// Handles are entities, values are plain numbers, and nothing here borrows
/// from the world after the call returns; the same shape as the C# callback
/// table and the Lua `fabr` table.
pub struct GameContext<'w> {
    world: &'w mut World,
}

impl<'w> GameContext<'w> {
    /// Reads an object's world translation.
    pub fn translation(&mut self, entity: Entity) -> Option<[f32; 3]> {
        let transform = self.world.get::<Transform>(entity)?;
        Some(transform.translation.to_array())
    }

    /// Writes an object's world translation; returns false for unknown objects.
    pub fn set_translation(&mut self, entity: Entity, translation: [f32; 3]) -> bool {
        let Some(mut transform) = self.world.get_mut::<Transform>(entity) else {
            return false;
        };
        transform.translation = Vec3::from_array(translation);
        true
    }

    /// Writes one line to the engine log.
    pub fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        eme::log::info!("{message}");
    }
}

/// The running game's instance, kept as a resource for the lifecycle systems.
#[derive(Resource)]
struct GameRuntime {
    game: Box<dyn FabrGame>,
}

/// Set once the host has seen an [`AppExit`](eme::app::AppExit).
#[derive(Resource, Default)]
struct GameStopRequested(bool);

fn start_game(world: &mut World) {
    world.resource_scope(|world, mut runtime: Mut<GameRuntime>| {
        let mut ctx = GameContext { world };
        runtime.game.on_start(&mut ctx);
    });
}

fn update_game(world: &mut World) {
    let dt = world.resource::<Time>().delta_secs();
    world.resource_scope(|world, mut runtime: Mut<GameRuntime>| {
        let mut ctx = GameContext { world };
        runtime.game.on_update(&mut ctx, dt);
    });
}

fn notice_game_exit(exit: MessageReader<AppExit>, mut stop: ResMut<GameStopRequested>) {
    if !exit.is_empty() {
        stop.0 = true;
    }
}

fn stop_game(world: &mut World) {
    if !world.resource::<GameStopRequested>().0 {
        return;
    }
    world.resource_scope(|world, mut runtime: Mut<GameRuntime>| {
        let mut ctx = GameContext { world };
        runtime.game.on_stop(&mut ctx);
    });
    world.resource_mut::<GameStopRequested>().0 = false;
}

/// Installs a game into a live app: its build hook plus the lifecycle systems.
pub fn install<G: FabrGame + Default>(app: &mut App) {
    let game = G::default();
    game.build(app);
    app.insert_resource(GameRuntime {
        game: Box::new(game),
    })
    .init_resource::<GameStopRequested>()
    .add_systems(Startup, start_game)
    .add_systems(Update, update_game)
    .add_systems(Last, (notice_game_exit, stop_game).chain());
}

/// Builds a game into a live host app.
///
/// # Safety
/// `app` must point at the host's live [`App`], built against the same engine
/// dylib and plugin ABI as the gameplay library.
pub unsafe fn register_game<G: FabrGame + Default>(app: *mut core::ffi::c_void) {
    // SAFETY: the caller guarantees the pointer is the host's App.
    let app = unsafe { &mut *app.cast::<App>() };
    install::<G>(app);
}

/// Exports the plugin ABI for a [`FabrGame`] implementation.
///
/// ```ignore
/// fabr::game_plugin!(MyGame);
/// ```
#[macro_export]
macro_rules! game_plugin {
    ($game:ty) => {
        /// Reports the plugin ABI this gameplay library implements.
        #[unsafe(no_mangle)]
        pub extern "C" fn fabr_plugin_abi() -> u32 {
            $crate::PLUGIN_ABI_VERSION
        }

        /// Registers the game into the host app.
        ///
        /// # Safety
        /// `app` must be the host's live App pointer.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn fabr_plugin_register(app: *mut core::ffi::c_void) {
            // SAFETY: the host passes its live App pointer and the library was
            // built against the same engine dylib.
            unsafe { $crate::register_game::<$game>(app) }
        }
    };
}

/// Version of the Fabr SDK surface.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Project settings plus the runtime input action map built from them.
#[derive(Clone, Debug)]
pub struct FabrConfig {
    /// Authored project settings.
    pub settings: ProjectSettings,
    /// Parsed actions ready for the runtime.
    pub input: InputActions,
}

impl Default for FabrConfig {
    fn default() -> Self {
        let settings = ProjectSettings::default();
        let input = InputActions::from_names(&settings.input.actions).unwrap_or_default();
        Self { settings, input }
    }
}

impl FabrConfig {
    /// Loads `fabr.project.ron` from `root`.
    ///
    /// A missing file yields defaults. A malformed file or an unknown key name
    /// is reported instead of silently disabling the action.
    pub fn load(root: &Path) -> Result<Self, String> {
        let settings = ProjectSettings::load_or_default(&root.join(SETTINGS_FILE))?;
        let input = InputActions::from_names(&settings.input.actions)?;
        Ok(Self { settings, input })
    }

    /// Writes the settings and validates the action map first.
    pub fn save(&self, root: &Path) -> Result<(), String> {
        let input = InputActions::from_names(&self.settings.input.actions)?;
        let _ = input;
        self.settings.save(&root.join(SETTINGS_FILE))
    }
}

/// Everything a Fabr game or tool normally imports.
pub mod prelude {
    pub use eme::prelude::*;

    pub use crate::{
        key_binding, script_files, script_property_defaults, FabrConfig, InputActions, InputSettings,
        NodeKey, ObjectKind, ObjectTransform, PlayerSettings, ProjectSettings, SceneDoc, SceneObject,
        ScriptLanguage, ScriptLog, ScriptPlugin, ScriptRef, ScriptRoot, ScriptValue,
        ScriptedObject, ScriptsActive, FabrGame, GameContext, SETTINGS_FILE, VERSION,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_ecs::system::RunSystemOnce;
    use crate::prelude::*;

    #[derive(Resource, Default)]
    struct GameMarker;

    struct TestGame;

    impl FabrGame for TestGame {
        fn build(&self, app: &mut App) {
            app.insert_resource(GameMarker);
            app.add_systems(Startup, || {});
        }
    }

    #[test]
    fn a_game_registers_resources_and_systems() {
        let mut app = App::new();
        TestGame.build(&mut app);
        assert!(app.world().contains_resource::<GameMarker>());
    }

    #[test]
    fn the_context_reads_and_writes_world_values() {
        let mut world = World::new();
        let entity = world
            .spawn(Transform::from_xyz(1.0, 2.0, 3.0))
            .id();
        let mut ctx = GameContext { world: &mut world };
        assert_eq!(ctx.translation(entity), Some([1.0, 2.0, 3.0]));
        assert!(ctx.set_translation(entity, [4.0, 5.0, 6.0]));
        assert_eq!(ctx.translation(entity), Some([4.0, 5.0, 6.0]));
        let missing = ctx.world.spawn_empty().id();
        ctx.world.despawn(missing);
        assert!(!ctx.set_translation(missing, [0.0; 3]));
    }

    #[test]
    fn lifecycle_hooks_run_in_order() {
        use std::sync::Mutex;
        static CALLS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        #[derive(Default)]
        struct Lifecycle;
        impl FabrGame for Lifecycle {
            fn on_start(&mut self, _ctx: &mut GameContext) {
                CALLS.lock().unwrap().push("start");
            }
            fn on_update(&mut self, _ctx: &mut GameContext, _dt: f32) {
                CALLS.lock().unwrap().push("update");
            }
            fn on_stop(&mut self, _ctx: &mut GameContext) {
                CALLS.lock().unwrap().push("stop");
            }
        }
        CALLS.lock().unwrap().clear();
        let mut app = App::new();
        app.insert_resource(Time::<()>::default());
        install::<Lifecycle>(&mut app);
        app.world_mut().run_system_once(start_game).unwrap();
        app.world_mut().run_system_once(update_game).unwrap();
        app.world_mut()
            .resource_mut::<GameStopRequested>()
            .0 = true;
        app.world_mut().run_system_once(stop_game).unwrap();
        assert_eq!(*CALLS.lock().unwrap(), ["start", "update", "stop"]);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-sdk-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_project_uses_defaults() {
        let dir = scratch("defaults");
        let config = FabrConfig::load(&dir).unwrap();
        assert_eq!(config.settings, ProjectSettings::default());
        assert_eq!(config.settings.graphics.msaa, 4);
        assert_eq!(config.settings.audio.master_volume, 1.0);
        assert_eq!(config.settings.audio.sfx_volume, 1.0);
        assert_eq!(config.settings.audio.music_volume, 1.0);
        assert!(config.settings.quality.shadows);
        assert_eq!(config.settings.physics.gravity, 9.81);
        assert!(config.settings.physics.enabled);
        assert!(config.input.map.contains_key("jump"));
    }

    #[test]
    fn project_settings_and_actions_load_together() {
        let dir = scratch("load");
        let settings = ProjectSettings::default();
        settings.save(&dir.join(SETTINGS_FILE)).unwrap();
        let config = FabrConfig::load(&dir).unwrap();
        assert_eq!(config.settings.player.title, "Fabr Game");
        assert!(config.input.map.contains_key("move_forward"));
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let dir = scratch("badkey");
        let mut settings = ProjectSettings::default();
        settings
            .input
            .actions
            .insert("shoot".into(), vec!["Banana".into()]);
        settings.save(&dir.join(SETTINGS_FILE)).unwrap();
        assert!(FabrConfig::load(&dir).is_err());
    }

    #[test]
    fn the_scene_types_are_reachable_through_the_sdk() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Cube,
            "Cube",
            ObjectTransform::from_translation([0.0; 3]),
        );
        assert!(doc.set_script(
            id,
            Some(ScriptRef {
                path: "scripts/spin.lua".into(),
                properties: Default::default(),
                ..Default::default()
            })
        ));
        assert!(doc.script(id).is_some());
    }
}
