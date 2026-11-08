//! Public Fabr game SDK.
//!
//! Game code should depend on this crate rather than the engine internals. It
//! exposes the stable authoring/runtime surface: scene documents, Lua scripts,
//! input actions and project settings. The editor and player use the same
//! underlying crates; this facade keeps their names and versions in one place.

pub use eme_editor_project::{InputSettings, PlayerSettings, ProjectSettings, SETTINGS_FILE};
pub use eme_game_scene::{
    AssetRef, Behaviour, BehaviourKind, NodeKey, NodeOverride, ObjectKind, ObjectTransform,
    OverrideOp, SceneDoc, SceneError, SceneObject, ScriptRef, ScriptValue, TransformDelta,
    FORMAT_VERSION,
};
pub use eme_script::{
    key_binding, script_files, script_property_defaults, update_scripts, AudioBus, AudioMix,
    InputActions, SceneRequests, ScriptLog, ScriptPlugin, ScriptReload, ScriptRoot,
    ScriptedObject, ScriptsActive,
};

use std::path::Path;

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
    pub use crate::{
        key_binding, script_files, script_property_defaults, FabrConfig, InputActions, InputSettings,
        NodeKey, ObjectKind, ObjectTransform, PlayerSettings, ProjectSettings, SceneDoc, SceneObject,
        ScriptLog, ScriptPlugin, ScriptRef, ScriptRoot, ScriptValue, ScriptedObject, ScriptsActive,
        SETTINGS_FILE, VERSION,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

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
            })
        ));
        assert!(doc.script(id).is_some());
    }
}
