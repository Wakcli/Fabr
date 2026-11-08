//! Optional engine extensions, toggled by the project's `fabr.plugins.ron`.

use alloc::collections::BTreeSet;

use eme_ecs::prelude::*;
use serde::{Deserialize, Serialize};

/// File listing enabled extensions.
pub const PLUGINS_FILE: &str = "fabr.plugins.ron";

/// One optional engine extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Extension {
    /// Lua gameplay scripts.
    #[default]
    Scripting,
    /// glTF animation playback.
    Animation,
    /// Client/server transform replication.
    Networking,
    /// Grid navigation and agents.
    Navigation,
    /// Gravity and AABB simulation.
    Physics,
}

impl Extension {
    /// Every extension, in display order.
    pub const ALL: [Extension; 5] = [
        Extension::Scripting,
        Extension::Animation,
        Extension::Networking,
        Extension::Navigation,
        Extension::Physics,
    ];

    /// Stable identifier used in the project file.
    pub fn id(self) -> &'static str {
        match self {
            Extension::Scripting => "scripting",
            Extension::Animation => "animation",
            Extension::Networking => "networking",
            Extension::Navigation => "navigation",
            Extension::Physics => "physics",
        }
    }

    /// Human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            Extension::Scripting => "Scripting",
            Extension::Animation => "Animation",
            Extension::Networking => "Networking",
            Extension::Navigation => "Navigation",
            Extension::Physics => "Physics",
        }
    }

    /// Extension for a stored identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|extension| extension.id() == id)
    }
}

/// One external plugin library listed in `fabr.plugins.ron`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ExternalPlugin {
    /// Display name.
    pub name: String,
    /// Library path, absolute or relative to the project root.
    pub library: String,
}

/// The project's enabled extension list.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct ExtensionSet {
    /// Identifiers of enabled extensions.
    pub enabled: BTreeSet<String>,
    /// External shared libraries loaded before the app starts.
    #[serde(default)]
    pub external: Vec<ExternalPlugin>,
}

impl Default for ExtensionSet {
    fn default() -> Self {
        Self {
            enabled: Extension::ALL.iter().map(|ext| ext.id().to_string()).collect(),
            external: Vec::new(),
        }
    }
}

impl ExtensionSet {
    /// Reads `fabr.plugins.ron`, falling back to all extensions enabled.
    pub fn load(path: &std::path::Path) -> Self {
        let Ok(src) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match ron::from_str::<Self>(&src) {
            Ok(set) if set.validate().is_ok() => set,
            Ok(set) => {
                let _ = set.validate();
                Self::default()
            }
            Err(_) => Self::default(),
        }
    }

    /// Writes the file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Whether `extension` is enabled.
    pub fn enabled(&self, extension: Extension) -> bool {
        self.enabled.contains(extension.id())
    }

    /// Enables or disables `extension`.
    pub fn set(&mut self, extension: Extension, on: bool) {
        if on {
            self.enabled.insert(extension.id().to_string());
        } else {
            self.enabled.remove(extension.id());
        }
    }

    /// Rejects unknown identifiers.
    pub fn validate(&self) -> Result<(), String> {
        for id in &self.enabled {
            if Extension::from_id(id).is_none() {
                return Err(format!("unknown extension {id:?}"));
            }
        }
        Ok(())
    }
}

/// Loads the project's extension set.
pub fn load_for(root: &std::path::Path) -> ExtensionSet {
    ExtensionSet::load(&root.join(PLUGINS_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_everything_and_validate() {
        let set = ExtensionSet::default();
        set.validate().unwrap();
        for extension in Extension::ALL {
            assert!(set.enabled(extension), "{} should default on", extension.id());
        }
    }

    #[test]
    fn toggling_writes_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("eme-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PLUGINS_FILE);
        let mut set = ExtensionSet::default();
        set.set(Extension::Scripting, false);
        set.set(Extension::Physics, false);
        set.save(&path).unwrap();
        let back = ExtensionSet::load(&path);
        assert!(!back.enabled(Extension::Scripting));
        assert!(!back.enabled(Extension::Physics));
        assert!(back.enabled(Extension::Animation));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_entries_are_rejected_on_load() {
        let dir = std::env::temp_dir().join(format!("eme-plugins-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PLUGINS_FILE);
        std::fs::write(&path, "(enabled: [\"scripting\", \"telepathy\"])").unwrap();
        let set = ExtensionSet::load(&path);
        assert!(set.enabled(Extension::Animation), "invalid file falls back to all");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_set_disables_everything() {
        let dir = std::env::temp_dir().join(format!("eme-plugins-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PLUGINS_FILE);
        std::fs::write(&path, "(enabled: [])").unwrap();
        let set = ExtensionSet::load(&path);
        for extension in Extension::ALL {
            assert!(!set.enabled(extension));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
