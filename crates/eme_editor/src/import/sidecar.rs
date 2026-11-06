//! Per-asset import settings stored next to the model.
//!
//! Bevy's `.meta` files belong to its asset processor: they carry hashes and
//! processor configuration that the engine rewrites, and this editor never runs
//! the processor. Import settings are authoring data — hand-editable and
//! diffable — so they get their own RON sidecar beside the asset.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::settings::ImportSettings;

/// Suffix appended to the model file name.
pub const SIDECAR_SUFFIX: &str = ".import.ron";
/// Schema version of the sidecar itself.
pub const SIDECAR_VERSION: u32 = 1;

/// Settings and identity for one imported asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sidecar {
    /// Sidecar schema version.
    pub version: u32,
    /// Stable identity that survives re-imports and file moves inside the project.
    pub guid: String,
    /// Options the model was imported with.
    pub settings: ImportSettings,
}

impl Sidecar {
    /// Where the sidecar for `asset` lives.
    pub fn path_for(asset: &Path) -> PathBuf {
        let mut name = asset
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "asset".into());
        name.push_str(SIDECAR_SUFFIX);
        asset.with_file_name(name)
    }

    /// Reads the sidecar for `asset`, or `None` when there is none or it is broken.
    pub fn load(asset: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(Self::path_for(asset)).ok()?;
        ron::from_str(&text).ok()
    }

    /// Reads the sidecar, or creates one with a fresh GUID.
    pub fn load_or_new(asset: &Path) -> Self {
        Self::load(asset).unwrap_or_else(|| Self {
            version: SIDECAR_VERSION,
            guid: new_guid(asset),
            settings: ImportSettings::default(),
        })
    }

    /// Writes the sidecar next to `asset`.
    pub fn save(&self, asset: &Path) -> Result<PathBuf, String> {
        let path = Self::path_for(asset);
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| format!("could not serialize {}: {err}", path.display()))?;
        std::fs::write(&path, text).map_err(|err| format!("could not write {}: {err}", path.display()))?;
        Ok(path)
    }
}

/// A GUID derived from the file path and the moment the sidecar was created.
///
/// It only has to be unique per asset and never change afterwards; the stored
/// value is what matters, so a simple hash is enough and adds no dependency.
fn new_guid(asset: &Path) -> String {
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    asset.hash(&mut hasher);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    stamp.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("{:016x}{:016x}", hasher.finish(), hasher.finish().rotate_left(17))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_asset(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-sidecar-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let asset = dir.join("chair.glb");
        std::fs::write(&asset, b"glTF").unwrap();
        asset
    }

    #[test]
    fn sidecar_lives_beside_the_asset() {
        let asset = temp_asset("path");
        assert_eq!(
            Sidecar::path_for(&asset),
            asset.with_file_name("chair.glb.import.ron")
        );
    }

    #[test]
    fn settings_round_trip_through_the_sidecar() {
        let asset = temp_asset("roundtrip");
        let mut sidecar = Sidecar::load_or_new(&asset);
        sidecar.settings.transform.scale = 0.01;
        sidecar.settings.materials.max_texture_size = 2048;
        sidecar.save(&asset).unwrap();

        let back = Sidecar::load(&asset).expect("sidecar loads");
        assert_eq!(back, sidecar);
    }

    #[test]
    fn the_guid_survives_reimports() {
        let asset = temp_asset("guid");
        let first = Sidecar::load_or_new(&asset);
        first.save(&asset).unwrap();

        let mut second = Sidecar::load_or_new(&asset);
        assert_eq!(second.guid, first.guid, "reopening must reuse the stored GUID");

        second.settings.geometry.colliders = true;
        second.save(&asset).unwrap();
        assert_eq!(Sidecar::load(&asset).unwrap().guid, first.guid);
    }

    #[test]
    fn a_broken_sidecar_is_ignored() {
        let asset = temp_asset("broken");
        std::fs::write(Sidecar::path_for(&asset), "not ron").unwrap();
        let fresh = Sidecar::load_or_new(&asset);
        assert_eq!(fresh.version, SIDECAR_VERSION);
        assert!(Sidecar::load(&asset).is_none());
    }
}
