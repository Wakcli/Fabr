//! Project layer tests.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use eme_editor_core::{HubState, Project, ProjectId};
use eme_editor_project::{
    create_project, load_session_state, load_user_state, open_project, save_session_state,
    save_user_state, user_config_dir, Manifest, ProjectError, SessionState, MANIFEST_FILE,
    SCHEMA_VERSION,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static DIRS: AtomicU64 = AtomicU64::new(0);

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let seq = DIRS.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "eme-editor-project-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct EnvVar {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvVar {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let prev = std::env::var_os(key);
        set_var(key, Some(value.as_ref()));
        Self { key, prev }
    }

    fn unset(key: &'static str) -> Self {
        let prev = std::env::var_os(key);
        set_var(key, None);
        Self { key, prev }
    }
}

impl Drop for EnvVar {
    fn drop(&mut self) {
        set_var(self.key, self.prev.as_deref());
    }
}

#[expect(
    unsafe_code,
    reason = "env vars are process-wide; env tests hold ENV_LOCK"
)]
fn set_var(key: &str, value: Option<&OsStr>) {
    // SAFETY: env tests are serialized by ENV_LOCK, so no other thread reads or
    // writes the process environment while the guard is held.
    unsafe {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

fn manifest(name: &str) -> Manifest {
    Manifest {
        schema: SCHEMA_VERSION,
        id: 7,
        name: name.to_string(),
        engine_version: "0.20.0".to_string(),
        created: 1_700_000_000,
        asset_root: "assets".to_string(),
        entry_scene: None,
        tags: Vec::new(),
    }
}

fn project(id: u64, name: &str, root: &str, opened: Option<u64>) -> Project {
    Project {
        id: ProjectId(id),
        name: name.to_string(),
        root: PathBuf::from(root),
        last_opened: opened,
        engine_version: "0.20.0".to_string(),
    }
}

#[test]
fn manifest_round_trips() {
    let dir = TempDir::new("manifest");
    let path = dir.join(MANIFEST_FILE);

    let mut manifest = manifest("Keep");
    manifest.entry_scene = Some("scenes/main.ron".to_string());
    manifest.tags = vec!["2d".to_string(), "demo".to_string()];

    manifest.save(&path).unwrap();
    assert_eq!(Manifest::load(&path).unwrap(), manifest);
}

#[test]
fn create_then_open_round_trips() {
    let dir = TempDir::new("create");
    let root = dir.join("Castle");

    let project = create_project(&root, "Castle", "0.20.0", 1_700_000_000, false).unwrap();

    assert!(root.join("assets").is_dir());
    assert!(root.join("scenes").is_dir());
    assert!(root.join(MANIFEST_FILE).is_file());
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        "target/\n"
    );
    assert_eq!(project.name, "Castle");
    assert_eq!(project.last_opened, Some(1_700_000_000));

    let (opened, loaded) = open_project(&root).unwrap();
    assert_eq!(opened.id, project.id);
    assert_eq!(opened.name, "Castle");
    assert_eq!(opened.root, root);
    assert_eq!(opened.engine_version, "0.20.0");
    assert_eq!(opened.last_opened, None);
    assert_eq!(loaded, Manifest::from_project(&project, "0.20.0"));
    assert_eq!(loaded.schema, SCHEMA_VERSION);
    assert_eq!(loaded.created, 1_700_000_000);
    assert_eq!(loaded.asset_root, "assets");
    assert_eq!(loaded.entry_scene, None);
}

#[test]
fn existing_directory_needs_overwrite() {
    let dir = TempDir::new("overwrite");

    let empty = dir.join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(create_project(&empty, "Empty", "0.20.0", 5, false).is_ok());

    let taken = dir.join("Taken");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("keep.txt"), "hi").unwrap();

    let err = create_project(&taken, "Taken", "0.20.0", 5, false).unwrap_err();
    assert!(matches!(err, ProjectError::Io(_)));
    assert!(!taken.join(MANIFEST_FILE).exists());

    create_project(&taken, "Taken", "0.20.0", 5, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(taken.join("keep.txt")).unwrap(),
        "hi"
    );
    assert!(taken.join(MANIFEST_FILE).is_file());
}

#[test]
fn names_are_validated() {
    let dir = TempDir::new("names");
    let long = "a".repeat(65);
    let bad = ["", "a/b", "a\\b", "b\u{7}ad", long.as_str()];

    for (idx, name) in bad.into_iter().enumerate() {
        let root = dir.join(&format!("bad{idx}"));
        let err = create_project(&root, name, "0.20.0", 0, false).unwrap_err();
        assert!(
            matches!(err, ProjectError::InvalidName(_)),
            "{name:?} accepted"
        );
        assert!(!root.exists(), "{name:?} created a directory");
    }

    let ok = "a".repeat(64);
    assert!(create_project(&dir.join("ok"), &ok, "0.20.0", 0, false).is_ok());
}

#[test]
fn open_rejects_plain_directories() {
    let dir = TempDir::new("plain");
    let err = open_project(dir.path()).unwrap_err();
    assert!(matches!(err, ProjectError::NotAProject));
}

#[test]
fn open_rejects_newer_schema() {
    let dir = TempDir::new("schema");
    let mut manifest = manifest("Newer");
    manifest.schema = 99;
    manifest.save(&dir.join(MANIFEST_FILE)).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();

    let err = open_project(dir.path()).unwrap_err();
    assert!(matches!(err, ProjectError::UnsupportedSchema(99)));
}

#[test]
fn open_requires_asset_root() {
    let dir = TempDir::new("assets");
    manifest("NoAssets").save(&dir.join(MANIFEST_FILE)).unwrap();

    let err = open_project(dir.path()).unwrap_err();
    assert!(matches!(err, ProjectError::MissingAssetRoot));
}

#[test]
fn open_requires_entry_scene() {
    let dir = TempDir::new("scene");
    std::fs::create_dir_all(dir.join("assets")).unwrap();

    let mut manifest = manifest("Scene");
    manifest.entry_scene = Some("scenes/main.ron".to_string());
    manifest.save(&dir.join(MANIFEST_FILE)).unwrap();

    let err = open_project(dir.path()).unwrap_err();
    assert!(matches!(err, ProjectError::MissingEntryScene));

    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(dir.join("scenes/main.ron"), "()").unwrap();

    let (project, loaded) = open_project(dir.path()).unwrap();
    assert_eq!(loaded.entry_scene.as_deref(), Some("scenes/main.ron"));
    assert_eq!(project.name, "Scene");
}

#[test]
fn user_state_round_trips() {
    let _lock = lock_env();
    let dir = TempDir::new("user");
    let _cfg = EnvVar::set("EMBERRLY_CONFIG_DIR", dir.path());
    assert_eq!(user_config_dir(), dir.path());

    let mut hub = HubState::new();
    hub.add_or_touch(project(3, "Alpha", "/w/alpha", Some(9)));
    hub.add_or_touch(project(4, "Beta", "/w/beta", Some(8)));
    hub.selected = Some(ProjectId(3));
    hub.set_filter("al");

    save_user_state(&hub).unwrap();
    assert!(dir.join("user.ron").is_file());
    assert_eq!(load_user_state(), hub);

    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files.len(), 1, "temp file left behind");
}

#[test]
fn corrupt_state_is_empty_and_survives() {
    let _lock = lock_env();
    let dir = TempDir::new("corrupt");
    let _cfg = EnvVar::set("EMBERRLY_CONFIG_DIR", dir.path());

    assert_eq!(load_user_state(), HubState::default());
    assert_eq!(load_session_state(), SessionState::default());

    std::fs::write(dir.join("user.ron"), "not ron").unwrap();
    std::fs::write(dir.join("session.ron"), "(((").unwrap();
    assert_eq!(load_user_state(), HubState::default());
    assert_eq!(load_session_state(), SessionState::default());
}

#[test]
fn session_state_round_trips() {
    let _lock = lock_env();
    let dir = TempDir::new("session");
    let _cfg = EnvVar::set("EMBERRLY_CONFIG_DIR", dir.path());

    let state = SessionState {
        layout: Some(PathBuf::from("/cfg/layout.ron")),
        theme: Some(PathBuf::from("/cfg/theme.ron")),
    };
    save_session_state(&state).unwrap();
    assert!(dir.join("session.ron").is_file());
    assert_eq!(load_session_state(), state);

    let empty = SessionState::default();
    save_session_state(&empty).unwrap();
    assert_eq!(load_session_state(), empty);
}

#[test]
fn config_dir_follows_env_precedence() {
    let _lock = lock_env();
    let dir = TempDir::new("cfgdir");
    let home = dir.join("home");
    let xdg = dir.join("xdg");
    let own = dir.join("own");

    let _home = EnvVar::set("HOME", home.as_os_str());
    let _no_own = EnvVar::unset("EMBERRLY_CONFIG_DIR");
    let _no_xdg = EnvVar::unset("XDG_CONFIG_HOME");
    assert_eq!(user_config_dir(), home.join(".config").join("emberrly"));

    let _xdg = EnvVar::set("XDG_CONFIG_HOME", xdg.as_os_str());
    assert_eq!(user_config_dir(), xdg.join("emberrly"));

    let _own = EnvVar::set("EMBERRLY_CONFIG_DIR", own.as_os_str());
    assert_eq!(user_config_dir(), own);
}
