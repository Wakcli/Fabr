use eme_app::{plugin_group, Plugin};

plugin_group! {
    /// This plugin group will add all the default plugins for a *Eme* application:
    pub struct DefaultPlugins {
        eme_app:::PanicHandlerPlugin,
        #[cfg(feature = "eme_log")]
        eme_log:::LogPlugin,
        eme_app:::TaskPoolPlugin,
        eme_diagnostic:::FrameCountPlugin,
        eme_time:::TimePlugin,
        eme_transform:::TransformPlugin,
        eme_diagnostic:::DiagnosticsPlugin,
        eme_input:::InputPlugin,
        #[cfg(feature = "eme_input_focus")]
        eme_input_focus:::InputFocusPlugin,
        #[cfg(feature = "eme_input_focus")]
        eme_input_focus:::InputDispatchPlugin,
        #[custom(cfg(not(feature = "eme_window")))]
        eme_app:::ScheduleRunnerPlugin,
        #[cfg(feature = "eme_window")]
        eme_window:::WindowPlugin,
        #[cfg(feature = "eme_window")]
        eme_a11y:::AccessibilityPlugin,
        #[cfg(feature = "std")]
        #[custom(cfg(any(all(unix, not(target_os = "horizon")), windows)))]
        eme_app:::TerminalCtrlCHandlerPlugin,
        // NOTE: Load this before AssetPlugin to properly register http asset sources.
        #[cfg(feature = "eme_asset")]
        #[custom(cfg(any(feature = "http", feature = "https")))]
        eme_asset::io::web:::WebAssetPlugin,
        #[cfg(feature = "eme_asset")]
        eme_asset:::AssetPlugin,
        #[cfg(feature = "eme_world_serialization")]
        eme_world_serialization:::WorldSerializationPlugin,
        #[cfg(feature = "eme_scene")]
        eme_scene:::ScenePlugin,
        // NOTE: WinitPlugin needs to be after AssetPlugin because of custom cursors.
        #[cfg(feature = "eme_winit")]
        eme_winit:::WinitPlugin,
        #[custom(cfg(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
        eme_anti_alias::dlss:::DlssInitPlugin,
        #[cfg(feature = "eme_render")]
        eme_render:::RenderPlugin,
        // NOTE: Load this after renderer initialization so that it knows about the supported
        // compressed texture formats.
        #[cfg(feature = "eme_image")]
        eme_image:::ImagePlugin,
        #[cfg(feature = "eme_mesh")]
        eme_mesh:::MeshPlugin,
        #[cfg(feature = "eme_camera")]
        eme_camera:::CameraPlugin,
        #[cfg(feature = "eme_light")]
        eme_light:::LightPlugin,
        #[cfg(feature = "eme_render")]
        #[custom(cfg(all(not(target_arch = "wasm32"), feature = "multi_threaded")))]
        eme_render::pipelined_rendering:::PipelinedRenderingPlugin,
        #[cfg(feature = "eme_core_pipeline")]
        eme_core_pipeline:::CorePipelinePlugin,
        #[cfg(feature = "eme_post_process")]
        eme_post_process:::PostProcessPlugin,
        #[cfg(feature = "eme_anti_alias")]
        eme_anti_alias:::AntiAliasPlugin,
        #[cfg(feature = "eme_sprite")]
        eme_sprite:::SpritePlugin,
        #[cfg(feature = "eme_sprite_render")]
        eme_sprite_render:::SpriteRenderPlugin,
        #[cfg(feature = "eme_clipboard")]
        eme_clipboard:::ClipboardPlugin,
        #[cfg(feature = "eme_text")]
        eme_text:::TextPlugin,
        #[cfg(feature = "eme_ui")]
        eme_ui:::UiPlugin,
        #[cfg(feature = "eme_ui_render")]
        eme_ui_render:::UiRenderPlugin,
        #[cfg(feature = "eme_gltf")]
        eme_gltf:::GltfPlugin,
        #[cfg(feature = "eme_pbr")]
        eme_pbr:::PbrPlugin,
        #[cfg(feature = "eme_audio")]
        eme_audio:::AudioPlugin,
        #[cfg(feature = "eme_gilrs")]
        eme_gilrs:::GilrsPlugin,
        #[cfg(feature = "eme_animation")]
        eme_animation:::AnimationPlugin,
        #[cfg(feature = "eme_gizmos")]
        eme_gizmos:::GizmoPlugin,
        #[cfg(feature = "eme_gizmos_render")]
        eme_gizmos_render:::GizmoRenderPlugin,
        #[cfg(feature = "eme_state")]
        eme_state::app:::StatesPlugin,
        #[cfg(feature = "eme_ci_testing")]
        eme_dev_tools::ci_testing:::CiTestingPlugin,
        #[cfg(feature = "render_dev_tools")]
        eme_dev_tools::render_debug:::RenderDebugOverlayPlugin,
        #[cfg(feature = "hotpatching")]
        eme_app::hotpatch:::HotPatchPlugin,
        #[plugin_group]
        #[cfg(feature = "eme_ui_widgets")]
        eme_ui_widgets:::UiWidgetsPlugins,
        #[plugin_group]
        #[cfg(feature = "eme_picking")]
        eme_picking:::DefaultPickingPlugins,
        #[doc(hidden)]
        :IgnoreAmbiguitiesPlugin,
    }
    /// [`DefaultPlugins`] obeys *Cargo* *feature* flags. Users may exert control over this plugin group
    /// by disabling `default-features` in their `Cargo.toml` and enabling only those features
    /// that they wish to use.
    ///
    /// [`DefaultPlugins`] contains all the plugins typically required to build
    /// a *Eme* application which includes a *window* and presentation components.
    /// For the absolute minimum number of plugins needed to run a Eme application, see [`MinimalPlugins`].
}

#[derive(Default)]
struct IgnoreAmbiguitiesPlugin;

impl Plugin for IgnoreAmbiguitiesPlugin {
    #[expect(
        clippy::allow_attributes,
        reason = "`unused_variables` is not always linted"
    )]
    #[allow(
        unused_variables,
        reason = "The `app` parameter is used only if a combination of crates that contain ambiguities with each other are enabled."
    )]
    fn build(&self, app: &mut eme_app::App) {
        #[cfg(all(feature = "eme_ui_widgets", feature = "eme_sprite"))]
        if app.is_plugin_added::<eme_ui_widgets::TextInputPlugin>()
            && app.is_plugin_added::<eme_sprite::SpritePlugin>()
        {
            // update_ime_position reads Window to reposition the IME cursor, while
            // update_text2d_layout writes Window bounds for text wrapping.
            app.ignore_ambiguity(
                eme_app::PostUpdate,
                eme_ui_widgets::ImeSystems::UpdatePosition,
                eme_sprite::update_text2d_layout,
            );
        }

        // eme_ui owns the Transform and cannot be animated
        #[cfg(all(feature = "eme_animation", feature = "eme_ui"))]
        if app.is_plugin_added::<eme_animation::AnimationPlugin>()
            && app.is_plugin_added::<eme_ui::UiPlugin>()
        {
            app.ignore_ambiguity(
                eme_app::PostUpdate,
                eme_animation::advance_animations,
                eme_ui::ui_layout_system,
            );
            app.ignore_ambiguity(
                eme_app::PostUpdate,
                eme_animation::animate_targets,
                eme_ui::ui_layout_system,
            );
        }
    }
}

plugin_group! {
    /// This plugin group will add the minimal plugins for a *Eme* application:
    pub struct MinimalPlugins {
        eme_app:::TaskPoolPlugin,
        eme_diagnostic:::FrameCountPlugin,
        eme_time:::TimePlugin,
        eme_app:::ScheduleRunnerPlugin,
        #[cfg(feature = "eme_ci_testing")]
        eme_dev_tools::ci_testing:::CiTestingPlugin,
    }
    /// This plugin group represents the absolute minimum, bare-bones, eme application.
    /// Use this if you want to have absolute control over the plugins used.
    ///
    /// It includes a [schedule runner (`ScheduleRunnerPlugin`)](crate::app::ScheduleRunnerPlugin)
    /// to provide functionality that would otherwise be driven by a windowed application's
    /// *event loop* or *message loop*.
    ///
    /// By default, this loop will run as fast as possible, which can result in high CPU usage.
    /// You can add a delay using [`run_loop`](crate::app::ScheduleRunnerPlugin::run_loop),
    /// or remove the loop using [`run_once`](crate::app::ScheduleRunnerPlugin::run_once).
    /// # Example:
    /// ```rust, no_run
    /// # use std::time::Duration;
    /// # use eme_app::{App, PluginGroup, ScheduleRunnerPlugin};
    /// # use eme_internal::MinimalPlugins;
    /// App::new().add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
    ///     // Run 60 times per second.
    ///     Duration::from_secs_f64(1.0 / 60.0),
    /// ))).run();
}
