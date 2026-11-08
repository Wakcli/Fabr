//! Project-level settings written beside the scene and read by the player.

extern crate alloc;

use alloc::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// File name of the Fabr project settings.
pub const SETTINGS_FILE: &str = "fabr.project.ron";

/// Settings that affect a built game.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// Window and player configuration.
    pub player: PlayerSettings,
    /// Renderer configuration.
    pub graphics: GraphicsSettings,
    /// Named input actions and their key bindings.
    pub input: InputSettings,
}

/// Window configuration for the standalone player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerSettings {
    /// Window title.
    pub title: String,
    /// Window width in physical pixels.
    pub width: u32,
    /// Window height in physical pixels.
    pub height: u32,
    /// Whether vsync is requested.
    pub vsync: bool,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            title: "Fabr Game".into(),
            width: 1280,
            height: 720,
            vsync: true,
        }
    }
}

/// Renderer configuration applied by the player.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    /// MSAA sample count: 1, 2, 4 or 8.
    pub msaa: u32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self { msaa: 4 }
    }
}

/// Named input actions used by scripts and gameplay code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSettings {
    /// Action name to key names (`"W"`, `"Space"`, `"ArrowUp"`, `"MouseLeft"`).
    pub actions: BTreeMap<String, Vec<String>>,
}

impl Default for InputSettings {
    fn default() -> Self {
        let mut actions = BTreeMap::new();
        actions.insert("jump".into(), vec!["Space".into()]);
        actions.insert("move_forward".into(), vec!["W".into(), "ArrowUp".into()]);
        actions.insert("move_back".into(), vec!["S".into(), "ArrowDown".into()]);
        actions.insert("move_left".into(), vec!["A".into(), "ArrowLeft".into()]);
        actions.insert("move_right".into(), vec!["D".into(), "ArrowRight".into()]);
        Self { actions }
    }
}

impl ProjectSettings {
    /// Reads settings at `path`, or returns defaults when the file is absent.
    pub fn load_or_default(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let src = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let settings: Self = ron::from_str(&src).map_err(|err| format!("{}: {err}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Writes settings to `path`.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Rejects empty titles, zero sizes and empty input actions.
    pub fn validate(&self) -> Result<(), String> {
        if self.player.title.trim().is_empty() {
            return Err("player title cannot be empty".into());
        }
        if self.player.width == 0 || self.player.height == 0 {
            return Err("player width and height must be positive".into());
        }
        if self.input.actions.values().any(Vec::is_empty) {
            return Err("input actions cannot have an empty key list".into());
        }
        if !matches!(self.graphics.msaa, 1 | 2 | 4 | 8) {
            return Err("MSAA must be 1, 2, 4 or 8 samples".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_round_trip() {
        let settings = ProjectSettings::default();
        assert!(settings.validate().is_ok());
        let text = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default()).unwrap();
        let back: ProjectSettings = ron::from_str(&text).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn a_missing_file_is_the_default() {
        let path = std::env::temp_dir().join("fabr-no-settings-here.ron");
        let _ = std::fs::remove_file(&path);
        assert_eq!(ProjectSettings::load_or_default(&path).unwrap(), ProjectSettings::default());
    }

    #[test]
    fn graphics_settings_are_validated() {
        let mut settings = ProjectSettings::default();
        settings.graphics.msaa = 3;
        assert!(settings.validate().is_err());
        settings.graphics.msaa = 8;
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let mut settings = ProjectSettings::default();
        settings.player.width = 0;
        assert!(settings.validate().is_err());
        settings.player.width = 800;
        settings.input.actions.insert("shoot".into(), Vec::new());
        assert!(settings.validate().is_err());
    }
}
