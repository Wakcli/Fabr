//! Local packages: folders with a `fabr.package.ron` manifest that contribute
//! assets to the project and to exported builds.

use alloc::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File listing the project's packages, relative to the project root.
pub const PACKAGES_FILE: &str = "fabr.packages.ron";
/// Resolved dependency lock file, relative to the project root.
pub const LOCK_FILE: &str = "fabr.lock";
/// Manifest file inside a package folder.
pub const PACKAGE_MANIFEST_FILE: &str = "fabr.package.ron";
/// Index file of a package registry folder.
pub const REGISTRY_FILE: &str = "fabr.registry.ron";

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

/// One dependency declared by a package.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// Package name.
    pub name: String,
    /// Semver requirement: `1.2.3`, `^1.2.0`, `~1.2.0` or `*`.
    pub version: String,
}

/// A parsed semantic version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major number.
    pub major: u32,
    /// Minor number.
    pub minor: u32,
    /// Patch number.
    pub patch: u32,
}

impl Version {
    /// Parses `major.minor.patch`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Whether this version satisfies `requirement`.
    pub fn matches(&self, requirement: &str) -> bool {
        let requirement = requirement.trim();
        if requirement == "*" {
            return true;
        }
        if let Some(rest) = requirement.strip_prefix('^') {
            return Version::parse(rest).is_some_and(|base| {
                self >= &base && self.major == base.major
            });
        }
        if let Some(rest) = requirement.strip_prefix('~') {
            return Version::parse(rest).is_some_and(|base| {
                self >= &base && self.major == base.major && self.minor == base.minor
            });
        }
        Version::parse(requirement).is_some_and(|exact| self == &exact)
    }
}

/// A registry folder's package index, read from [`REGISTRY_FILE`].
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct RegistryIndex {
    /// Packages the registry publishes.
    pub packages: Vec<RegistryPackage>,
}

impl RegistryIndex {
    /// Reads a registry index.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        ron::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// One package published by a registry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegistryPackage {
    /// Package name, matching its `fabr.package.ron`.
    pub name: String,
    /// Published versions, in any order.
    pub versions: Vec<RegistryVersion>,
}

/// One published version and the folder holding it inside the registry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegistryVersion {
    /// Semantic version, `major.minor.patch`.
    pub version: String,
    /// Folder inside the registry, for example `packages/art-1.1.0`.
    pub path: String,
}

/// One entry of `fabr.lock`, resolved from the installed packages.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LockedPackage {
    /// Package name.
    pub name: String,
    /// Resolved version.
    pub version: String,
    /// Folder relative to the project root.
    pub path: String,
}

/// Deterministic resolution of the installed packages and their dependencies.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Lockfile {
    /// Packages sorted by name.
    pub packages: Vec<LockedPackage>,
}

impl Lockfile {
    /// Reads `fabr.lock`; missing files produce an empty lock.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        ron::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Writes the lock file.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| err.to_string())?;
        std::fs::write(path, text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Package names in resolution order.
    pub fn names(&self) -> Vec<&str> {
        self.packages.iter().map(|entry| entry.name.as_str()).collect()
    }
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
    /// Packages this one depends on.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

impl Default for PackageManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: "0.1.0".into(),
            description: String::new(),
            assets: "assets".into(),
            dependencies: Vec::new(),
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

    /// Resolves every installed package and its transitive dependencies.
    ///
    /// Dependency names resolve against the installed packages; the result is
    /// sorted by name so builds and lock files are deterministic.
    pub fn resolve(&self, root: &Path) -> Result<Lockfile, String> {
        let mut installed: BTreeMap<String, (String, PackageManifest)> = BTreeMap::new();
        for entry in &self.packages {
            let manifest = Self::manifest(root, entry)?;
            if let Some((path, _)) = installed.get(&manifest.name)
                && path != &entry.path
            {
                return Err(format!(
                    "package {} is installed twice ({} and {})",
                    manifest.name, path, entry.path
                ));
            }
            installed.insert(manifest.name.clone(), (entry.path.clone(), manifest));
        }
        // transitive dependencies also resolve from the local registry folder
        if let Ok(entries) = std::fs::read_dir(root.join("packages")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = format!("packages/{name}");
                let candidate = PackageEntry { path };
                if let Ok(manifest) = Self::manifest(root, &candidate) {
                    installed
                        .entry(manifest.name.clone())
                        .or_insert((candidate.path, manifest));
                }
            }
        }
        let mut locked: BTreeMap<String, LockedPackage> = BTreeMap::new();
        let mut visiting: Vec<String> = Vec::new();
        for name in installed.keys() {
            visit(name, &installed, &mut locked, &mut visiting)?;
        }
        Ok(Lockfile {
            packages: locked.into_values().collect(),
        })
    }

    /// Removes the package at `index`.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.packages.len() {
            return false;
        }
        self.packages.remove(index);
        true
    }

    /// Installs a package from a registry folder into `packages/<name>`.
    ///
    /// The highest published version matching `requirement` is vendored into
    /// the project and added to `fabr.packages.ron`, so builds and exports do
    /// not need the registry afterwards.
    pub fn install_from_registry(
        &mut self,
        root: &Path,
        registry: &Path,
        name: &str,
        requirement: &str,
    ) -> Result<PackageManifest, String> {
        let index = RegistryIndex::load(&registry.join(REGISTRY_FILE))?;
        let package = index
            .packages
            .iter()
            .find(|package| package.name == name)
            .ok_or_else(|| format!("{} has no package {name}", registry.display()))?;
        let (version, published) = package
            .versions
            .iter()
            .filter_map(|published| {
                Version::parse(&published.version).map(|version| (version, published))
            })
            .filter(|(version, _)| version.matches(requirement))
            .max_by_key(|(version, _)| *version)
            .ok_or_else(|| {
                format!("{name} has no version matching {requirement:?} in {}", registry.display())
            })?;
        if published.path.split('/').any(|part| part == "..") || published.path.starts_with('/') {
            return Err(format!("{name} {} has an unsafe folder", published.version));
        }
        let source = registry.join(&published.path);
        let manifest_path = source.join(PACKAGE_MANIFEST_FILE);
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|err| format!("{}: {err}", manifest_path.display()))?;
        let manifest: PackageManifest = ron::from_str(&text)
            .map_err(|err| format!("{}: {err}", manifest_path.display()))?;
        manifest.validate()?;
        if manifest.name != name {
            return Err(format!(
                "{} publishes {name} but its manifest names {}",
                source.display(),
                manifest.name
            ));
        }
        if Version::parse(&manifest.version) != Some(version) {
            return Err(format!(
                "{name} {} has manifest version {}",
                published.version, manifest.version
            ));
        }

        let dir = format!("packages/{name}");
        let destination = root.join(&dir);
        if destination.exists() {
            let existing = Self::manifest(
                root,
                &PackageEntry {
                    path: dir.clone(),
                },
            )
            .ok();
            if existing.as_ref().is_some_and(|installed| {
                installed.name == manifest.name && installed.version == manifest.version
            }) {
                self.remember(dir);
                return Ok(manifest);
            }
            std::fs::remove_dir_all(&destination)
                .map_err(|err| format!("{}: {err}", destination.display()))?;
        }
        copy_registry_dir(&source, &destination)?;
        self.remember(dir);
        Ok(manifest)
    }

    fn remember(&mut self, dir: String) {
        if !self.packages.iter().any(|entry| entry.path == dir) {
            self.packages.push(PackageEntry { path: dir });
        }
    }
}

fn copy_registry_dir(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|err| format!("{}: {err}", destination.display()))?;
    let entries =
        std::fs::read_dir(source).map_err(|err| format!("{}: {err}", source.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let metadata = entry
            .metadata()
            .map_err(|err| format!("{}: {err}", path.display()))?;
        if metadata.is_dir() {
            copy_registry_dir(&path, &target)?;
        } else if metadata.is_file() {
            std::fs::copy(&path, &target).map_err(|err| format!("{}: {err}", path.display()))?;
        }
    }
    Ok(())
}

fn visit(
    name: &str,
    installed: &BTreeMap<String, (String, PackageManifest)>,
    locked: &mut BTreeMap<String, LockedPackage>,
    visiting: &mut Vec<String>,
) -> Result<(), String> {
    if locked.contains_key(name) {
        return Ok(());
    }
    if visiting.iter().any(|seen| seen == name) {
        return Err(format!(
            "dependency cycle: {} -> {name}",
            visiting.join(" -> ")
        ));
    }
    let Some((path, manifest)) = installed.get(name) else {
        return Err(format!("package {name} is not installed"));
    };
    let version = Version::parse(&manifest.version)
        .ok_or_else(|| format!("{name} has invalid version {:?}", manifest.version))?;
    visiting.push(name.to_string());
    for dependency in &manifest.dependencies {
        if dependency.version.trim().is_empty() {
            return Err(format!("{name} depends on {} without a version", dependency.name));
        }
        let Some((_, target)) = installed.get(&dependency.name) else {
            return Err(format!(
                "{name} depends on {}, which is not installed",
                dependency.name
            ));
        };
        let target_version = Version::parse(&target.version)
            .ok_or_else(|| format!("{} has invalid version {:?}", target.name, target.version))?;
        if !target_version.matches(&dependency.version) {
            return Err(format!(
                "{name} needs {} {}, but {} is installed",
                dependency.name, dependency.version, target.version
            ));
        }
        visit(&dependency.name, installed, locked, visiting)?;
    }
    visiting.pop();
    locked.insert(
        name.to_string(),
        LockedPackage {
            name: name.to_string(),
            version: format!("{}.{}.{}", version.major, version.minor, version.patch),
            path: path.clone(),
        },
    );
    Ok(())
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
    fn a_registry_package_is_vendored_at_the_matching_version() {
        let root = scratch("registry-project");
        let registry = scratch("registry-index");
        package(
            &registry,
            "packages/art-1.0.0",
            r#"(name: "art", version: "1.0.0", assets: "assets")"#,
        );
        package(
            &registry,
            "packages/art-1.2.0",
            r#"(name: "art", version: "1.2.0", assets: "assets")"#,
        );
        std::fs::write(
            registry.join("packages/art-1.2.0/assets/tex.txt"),
            b"art",
        )
        .unwrap();
        std::fs::write(
            registry.join(REGISTRY_FILE),
            r#"(
                packages: [
                    (name: "art", versions: [
                        (version: "1.0.0", path: "packages/art-1.0.0"),
                        (version: "1.2.0", path: "packages/art-1.2.0"),
                    ]),
                ],
            )"#,
        )
        .unwrap();

        let mut set = PackageSet::default();
        let manifest = set
            .install_from_registry(&root, &registry, "art", "^1.0.0")
            .unwrap();
        assert_eq!(manifest.version, "1.2.0");
        assert_eq!(set.packages[0].path, "packages/art");
        assert_eq!(
            std::fs::read(root.join("packages/art/assets/tex.txt")).unwrap(),
            b"art"
        );
        let resolved = set.resolve(&root).unwrap();
        assert_eq!(resolved.names(), ["art"]);

        set.install_from_registry(&root, &registry, "art", "1.2.0")
            .unwrap();
        assert_eq!(set.packages.len(), 1);
        assert!(
            set.install_from_registry(&root, &registry, "art", "^2.0.0")
                .is_err()
        );
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&registry).ok();
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
    fn versions_match_ranges() {
        let version = Version::parse("1.4.2").unwrap();
        assert!(version.matches("1.4.2"));
        assert!(!version.matches("1.4.3"));
        assert!(version.matches("^1.2.0"));
        assert!(!version.matches("^2.0.0"));
        assert!(version.matches("~1.4.0"));
        assert!(!version.matches("~1.3.0"));
        assert!(version.matches("*"));
        assert!(Version::parse("1.2").is_none());
    }

    #[test]
    fn resolution_follows_transitive_dependencies() {
        let root = scratch("resolve");
        package(
            &root,
            "packages/base",
            r#"(name: "base", version: "1.0.0")"#,
        );
        package(
            &root,
            "packages/art",
            r#"(name: "art", version: "1.2.0", dependencies: [(name: "base", version: "^1.0.0")])"#,
        );
        package(
            &root,
            "packages/game",
            r#"(name: "game", version: "2.0.0", dependencies: [(name: "art", version: "^1.2.0")])"#,
        );
        let mut set = PackageSet::default();
        set.add(&root, "packages/base").unwrap();
        set.add(&root, "packages/art").unwrap();
        set.add(&root, "packages/game").unwrap();
        let lock = set.resolve(&root).unwrap();
        assert_eq!(lock.names(), ["art", "base", "game"]);
        assert_eq!(lock.packages[1].version, "1.0.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolution_rejects_missing_and_mismatched_dependencies() {
        let root = scratch("resolve-bad");
        package(&root, "packages/base", r#"(name: "base", version: "1.0.0")"#);
        package(
            &root,
            "packages/game",
            r#"(name: "game", version: "1.0.0", dependencies: [(name: "art", version: "^1.0.0")])"#,
        );
        let mut set = PackageSet::default();
        set.add(&root, "packages/base").unwrap();
        set.add(&root, "packages/game").unwrap();
        let err = set.resolve(&root).unwrap_err();
        assert!(err.contains("not installed"), "{err}");

        package(&root, "packages/art", r#"(name: "art", version: "2.0.0")"#);
        set.packages.push(PackageEntry { path: "packages/art".into() });
        let err = set.resolve(&root).unwrap_err();
        assert!(err.contains("needs art"), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolution_rejects_cycles() {
        let root = scratch("resolve-cycle");
        package(&root, "packages/a", r#"(name: "a", version: "1.0.0", dependencies: [(name: "b", version: "1.0.0")])"#);
        package(&root, "packages/b", r#"(name: "b", version: "1.0.0", dependencies: [(name: "a", version: "1.0.0")])"#);
        let mut set = PackageSet::default();
        set.add(&root, "packages/a").unwrap();
        set.add(&root, "packages/b").unwrap();
        let err = set.resolve(&root).unwrap_err();
        assert!(err.contains("dependency cycle"), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_lockfile_round_trips() {
        let root = scratch("lock");
        let lock = Lockfile {
            packages: vec![LockedPackage {
                name: "game".into(),
                version: "1.0.0".into(),
                path: "packages/game".into(),
            }],
        };
        let path = root.join(LOCK_FILE);
        lock.save(&path).unwrap();
        assert_eq!(Lockfile::load(&path).unwrap(), lock);
        assert_eq!(Lockfile::load(&root.join("missing.lock")).unwrap(), Lockfile::default());
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
