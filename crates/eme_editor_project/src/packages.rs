//! Local packages: folders with a `fabr.package.ron` manifest that contribute
//! assets to the project and to exported builds.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File listing the project's packages, relative to the project root.
pub const PACKAGES_FILE: &str = "fabr.packages.ron";
/// Manifest file inside a package folder.
pub const PACKAGE_MANIFEST_FILE: &str = "fabr.package.ron";

/// One package reference, relative to the project root.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PackageEntry {
    /// Folder containing `fabr.package.ron`.
    pub path: String,
}

/// The project's package list.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct PackageSet {
    /// Installed packages in display order.
    pub packages: Vec<PackageEntry>,
}

/// Contents of `fabr.package.ron`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct PackageManifest {
    /// Package name, unique within the project.
    pub name: String,
    /// Semantic version, `major.minor.patch`.
    pub version: String,
    /// One-line description.
    pub description: String,
    /// Folder inside the package whose contents are staged into `assets/`.
    pub assets: String,
}

impl Default for PackageManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.1.0".into(),
            description: String::new(),
            assets: "assets".into(),
        }
    }
}

impl PackageManifest {
    /// Problems that make the manifest unusable.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("package name must not be empty".into());
        }
        if !self.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("package name may only contain letters, digits, '-' and '_'".into());
        }
        if !valid_version(&self.version) {
            return Err(format!(
                "version {:?} must look like major.minor.patch",
                self.version
            ));
        }
        if self.assets.trim().is_empty() {
            return Err("assets folder must not be empty".into());
        }
        Ok(())
    }
}

/// Whether `version` is three dot-separated numbers.
pub fn valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

impl PackageSet {
    /// Reads `fabr.packages.ron`, or an empty set when it does not exist.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        ron::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Writes `fabr.packages.ron`.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Reads the manifest of `entry` inside `root`.
    pub fn manifest(root: &Path, entry: &PackageEntry) -> Result<PackageManifest, String> {
        let path = root.join(&entry.path).join(PACKAGE_MANIFEST_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        let manifest: PackageManifest =
            ron::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Adds the package in `dir` (relative to `root`) after validating it.
    pub fn add(&mut self, root: &Path, dir: &str) -> Result<PackageManifest, String> {
        let dir = dir.trim().trim_end_matches('/');
        if dir.is_empty() {
            return Err("enter a package folder, for example packages/my-pack".into());
        }
        let path = PathBuf::from(dir);
        if path.is_absolute() || dir.split('/').any(|part| part == "..") {
            return Err("package path must stay inside the project".into());
        }
        let entry = PackageEntry {
            path: dir.to_owned(),
        };
        if self.packages.iter().any(|known| known.path == entry.path) {
            return Err(format!("{dir} is already installed"));
        }
        let manifest = Self::manifest(root, &entry)?;
        if self
            .packages
            .iter()
            .filter_map(|known| Self::manifest(root, known).ok())
            .any(|known| known.name == manifest.name)
        {
            return Err(format!("a package named {} is already installed", manifest.name));
        }
        self.packages.push(entry);
        Ok(manifest)
    }

    /// Removes the package at `index`.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.packages.len() {
            return false;
        }
        self.packages.remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme-packages-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn package(root: &Path, dir: &str, manifest: &str) {
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(dir).join(PACKAGE_MANIFEST_FILE), manifest).unwrap();
        std::fs::create_dir_all(root.join(dir).join("assets")).unwrap();
    }

    #[test]
    fn versions_are_three_numbers() {
        assert!(valid_version("1.0.0"));
        assert!(valid_version("10.20.300"));
        assert!(!valid_version("1.0"));
        assert!(!valid_version("1.0.x"));
        assert!(!valid_version(""));
    }

    #[test]
    fn a_manifest_round_trips_through_ron() {
        let text = r#"(name: "sky", version: "1.2.3", description: "skybox pack", assets: "assets")"#;
        let manifest: PackageManifest = ron::from_str(text).unwrap();
        assert_eq!(manifest.name, "sky");
        assert_eq!(manifest.version, "1.2.3");
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn an_invalid_manifest_is_rejected() {
        let bad_version = PackageManifest {
            name: "sky".into(),
            version: "one".into(),
            ..Default::default()
        };
        assert!(bad_version.validate().is_err());
        let bad_name = PackageManifest {
            name: "sky pack".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };
        assert!(bad_name.validate().is_err());
        assert!(PackageManifest {
            name: "sky".into(),
            version: "1.0.0".into(),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn adding_a_package_validates_and_deduplicates() {
        let root = scratch("add");
        package(
            &root,
            "packages/sky",
            r#"(name: "sky", version: "1.0.0", assets: "assets")"#,
        );
        let mut set = PackageSet::default();
        let manifest = set.add(&root, "packages/sky").unwrap();
        assert_eq!(manifest.name, "sky");
        assert_eq!(set.packages.len(), 1);
        assert!(set.add(&root, "packages/sky").is_err());
        assert!(set.add(&root, "../outside").is_err());
        assert!(set.add(&root, "packages/missing").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn two_packages_cannot_share_a_name() {
        let root = scratch("names");
        package(&root, "packages/a", r#"(name: "sky", version: "1.0.0")"#);
        package(&root, "packages/b", r#"(name: "sky", version: "2.0.0")"#);
        let mut set = PackageSet::default();
        set.add(&root, "packages/a").unwrap();
        assert!(set.add(&root, "packages/b").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_set_round_trips_and_removes() {
        let root = scratch("roundtrip");
        let path = root.join(PACKAGES_FILE);
        let mut set = PackageSet::default();
        set.packages.push(PackageEntry {
            path: "packages/sky".into(),
        });
        set.save(&path).unwrap();
        let back = PackageSet::load(&path).unwrap();
        assert_eq!(back, set);
        let mut back = back;
        assert!(back.remove(0));
        assert!(!back.remove(0));
        assert!(back.packages.is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
