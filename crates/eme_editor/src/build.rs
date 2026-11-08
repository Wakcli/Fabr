//! Build: stages the saved scene and the runtime player into a game folder.
//!
//! Everything here runs off the UI thread. The dialog owns a [`BuildState`],
//! starts a worker, and drains its messages while the editor keeps rendering.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use eme::prelude::*;
use eme_game_scene::SceneDoc;

use crate::persist;

/// Platform the build produces a game for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Target {
    /// Native Linux folder, the only target this machine can assemble.
    #[default]
    Linux,
    /// Needs a CI template: the player must be compiled on Windows.
    Windows,
    /// Needs a CI template: the player must be compiled on macOS.
    MacOs,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Linux, Target::Windows, Target::MacOs];

    pub fn label(self) -> &'static str {
        match self {
            Target::Linux => "Linux",
            Target::Windows => "Windows",
            Target::MacOs => "macOS",
        }
    }

    /// Whether this machine can assemble the target end to end.
    pub fn ready(self) -> bool {
        matches!(self, Target::Linux)
    }
}

/// Cargo profile the staged player was built with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Profile {
    /// Optimized release player.
    #[default]
    Release,
    /// Faster to compile, slower to run.
    Debug,
}

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::Debug, Profile::Release];

    pub fn label(self) -> &'static str {
        match self {
            Profile::Release => "Release",
            Profile::Debug => "Debug",
        }
    }

    fn cargo_flags(self) -> &'static [&'static str] {
        match self {
            Profile::Release => &["--release"],
            Profile::Debug => &[],
        }
    }
}

/// Folder the game is written to, relative to the project root.
pub const OUT_DIR: &str = "build/linux";
/// Name the player binary takes in the game folder.
pub const GAME_BIN: &str = "emberrly-game";
/// Scene file the player looks for next to itself.
pub const SCENE_FILE: &str = "scene.ember";
/// Asset folder staged beside the player when the project has one.
pub const ASSET_DIR: &str = "assets";
/// Cargo package that provides the runtime player template.
pub const PLAYER_PKG: &str = "eme_player";
/// Longest build log the dialog keeps; a cold cargo build is chatty.
pub const MAX_LINES: usize = 400;

/// Severity of one log line, used to color it in the dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Step,
    Done,
    Error,
}

/// One line in the build log.
#[derive(Clone, Debug)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

/// What the worker reports back to the dialog.
enum Msg {
    Line(Level, String),
    Done(Result<PathBuf, String>),
}

/// Build dialog state: whether it is open, what ran, and where the game is.
#[derive(Resource, Default)]
pub struct BuildState {
    pub open: bool,
    pub running: bool,
    /// Cargo profile the next build uses.
    pub profile: Profile,
    pub log: Vec<Line>,
    /// Binary of the last successful build, ready to run.
    pub game: Option<PathBuf>,
    /// Bumped on every visible change so the dialog can rebuild.
    pub rev: u64,
    rx: Option<Mutex<Receiver<Msg>>>,
}

impl BuildState {
    pub fn open(&mut self) {
        if self.open {
            return;
        }
        self.open = true;
        self.rev += 1;
    }

    pub fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        self.rev += 1;
    }

    /// Selects the cargo profile used by the next build.
    pub fn set_profile(&mut self, profile: Profile) {
        if self.profile == profile {
            return;
        }
        self.profile = profile;
        self.rev += 1;
    }

    /// Starts a background build of `doc`; ignored while one is running.
    pub fn start(&mut self, doc: SceneDoc) {
        if self.running {
            return;
        }
        self.running = true;
        self.game = None;
        self.log.clear();
        if doc.objects.is_empty() {
            self.line(
                Level::Error,
                "warning: scene has no objects; the built game will be empty",
            );
        }
        self.rev += 1;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(Mutex::new(rx));
        let profile = self.profile;
        thread::spawn(move || run(doc, profile, tx));
    }

    /// Appends one log line, dropping the oldest past [`MAX_LINES`].
    pub fn line(&mut self, level: Level, text: impl Into<String>) {
        if self.log.len() == MAX_LINES {
            self.log.remove(0);
        }
        self.log.push(Line {
            level,
            text: text.into(),
        });
        self.rev += 1;
    }

    /// The status line under the log.
    pub fn status(&self) -> &'static str {
        if self.running {
            "Building..."
        } else if self.game.is_some() {
            "Build finished"
        } else if self.log.iter().any(|line| line.level == Level::Error) {
            "Build failed"
        } else {
            "Not built yet"
        }
    }
}

/// Drains worker messages into the dialog state.
pub fn poll_build(mut state: ResMut<BuildState>) {
    let msgs = {
        let Some(rx) = state.rx.as_ref() else {
            return;
        };
        let Ok(rx) = rx.lock() else {
            return;
        };
        rx.try_iter().collect::<Vec<Msg>>()
    };
    if msgs.is_empty() {
        return;
    }
    for msg in msgs {
        match msg {
            Msg::Line(level, text) => state.line(level, text),
            Msg::Done(result) => {
                state.running = false;
                state.rx = None;
                state.game = result.ok();
                state.rev += 1;
            }
        }
    }
}

fn run(doc: SceneDoc, profile: Profile, tx: Sender<Msg>) {
    match package(&doc, profile, &tx) {
        Ok(dir) => {
            let abs = std::fs::canonicalize(&dir).unwrap_or(dir);
            say(&tx, Level::Done, format!("game folder {}", abs.display()));
            let _ = tx.send(Msg::Done(Ok(abs.join(GAME_BIN))));
        }
        Err(err) => {
            say(&tx, Level::Error, format!("build failed: {err}"));
            let _ = tx.send(Msg::Done(Err(err)));
        }
    }
}

fn package(doc: &SceneDoc, profile: Profile, tx: &Sender<Msg>) -> Result<PathBuf, String> {
    let scene = persist::save_to(doc, persist::SCENE_PATH)
        .map_err(|err| format!("could not save scene: {err}"))?;
    say(tx, Level::Step, format!("saved scene {}", scene.display()));

    let target = target_dir();
    say(tx, Level::Step, format!("target dir {}", target.display()));
    let player = resolve_player(&target, profile, tx)?;

    let out = PathBuf::from(OUT_DIR);
    let assets = Path::new(ASSET_DIR);
    stage(&player, &scene, &out, assets.is_dir().then_some(assets), tx)?;
    Ok(out)
}

/// Picks the player binary, building it first when cargo has not produced one.
fn resolve_player(target: &Path, profile: Profile, tx: &Sender<Msg>) -> Result<PathBuf, String> {
    let [release, debug] = candidates(target);
    let chosen = match profile {
        Profile::Release => release,
        Profile::Debug => debug,
    };
    if chosen.is_file() {
        say(
            tx,
            Level::Step,
            format!("player {} build", profile.label().to_ascii_lowercase()),
        );
        return Ok(chosen);
    }
    say(
        tx,
        Level::Step,
        format!(
            "no player {} binary, running cargo build -p {PLAYER_PKG} {}",
            profile.label().to_ascii_lowercase(),
            profile.cargo_flags().join(" ")
        ),
    );
    build_player(tx, profile)?;
    if chosen.is_file() {
        say(tx, Level::Step, "player built");
        return Ok(chosen);
    }
    Err(format!(
        "cargo finished but {} is missing",
        chosen.display()
    ))
}

/// Player locations in preference order: release first, then debug.
fn candidates(target: &Path) -> [PathBuf; 2] {
    [
        target.join("release").join(player_name()),
        target.join("debug").join(player_name()),
    ]
}

fn build_player(tx: &Sender<Msg>, profile: Profile) -> Result<(), String> {
    let mut child = Command::new("cargo")
        .args(["build", "-p", PLAYER_PKG])
        .args(profile.cargo_flags())
        .current_dir(workspace())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not start cargo: {err}"))?;
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                say(tx, Level::Step, line);
            }
        }
    }
    let status = child
        .wait()
        .map_err(|err| format!("cargo build did not finish: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build exited with {status}"))
    }
}

/// Cleans the output folder and copies the player, scene, and assets into it.
fn stage(
    player: &Path,
    scene: &Path,
    out: &Path,
    assets: Option<&Path>,
    tx: &Sender<Msg>,
) -> Result<(), String> {
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|err| io(out, err))?;
        say(tx, Level::Step, format!("cleaned {OUT_DIR}"));
    }
    std::fs::create_dir_all(out).map_err(|err| io(out, err))?;

    let game = out.join(GAME_BIN);
    std::fs::copy(player, &game).map_err(|err| io(player, err))?;
    set_executable(&game)?;
    say(
        tx,
        Level::Step,
        format!("copied player to {}/{GAME_BIN}", OUT_DIR),
    );

    std::fs::copy(scene, out.join(SCENE_FILE)).map_err(|err| io(scene, err))?;
    say(
        tx,
        Level::Step,
        format!("copied scene to {OUT_DIR}/{SCENE_FILE}"),
    );

    match assets {
        Some(src) => {
            copy_dir(src, &out.join(ASSET_DIR))?;
            say(tx, Level::Step, format!("copied {ASSET_DIR}/"));
        }
        None => say(tx, Level::Step, "no assets/ folder to copy"),
    }

    let settings = workspace_dir().join(eme_editor_project::SETTINGS_FILE);
    if settings.is_file() {
        std::fs::copy(&settings, out.join(eme_editor_project::SETTINGS_FILE))
            .map_err(|err| io(&settings, err))?;
        say(tx, Level::Step, "copied project settings");
    }
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|err| io(dst, err))?;
    for entry in std::fs::read_dir(src).map_err(|err| io(src, err))? {
        let entry = entry.map_err(|err| io(src, err))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let kind = entry.file_type().map_err(|err| io(&from, err))?;
        if kind.is_dir() {
            copy_dir(&from, &to)?;
        } else if kind.is_file() {
            std::fs::copy(&from, &to).map_err(|err| io(&from, err))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map_err(|err| io(path, err))?
        .permissions()
        .mode()
        | 0o755;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|err| io(path, err))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// The freshest runtime player binary, if one has been built.
pub fn player_binary() -> Option<PathBuf> {
    candidates(&target_dir()).into_iter().find(|path| path.is_file())
}

/// Workspace root, used as the working directory for anything the editor spawns.
pub fn workspace_dir() -> PathBuf {
    workspace()
}

/// Git-ignored cache for import bookkeeping.
pub(crate) fn import_cache_dir() -> PathBuf {
    target_dir().join("import-cache")
}

/// Cargo's target directory, honoring `CARGO_TARGET_DIR` and `.cargo/config.toml`.
pub(crate) fn target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(dir);
    }
    metadata_target_dir().unwrap_or_else(|| PathBuf::from("target"))
}

fn metadata_target_dir() -> Option<PathBuf> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    const KEY: &str = "\"target_directory\":\"";
    let start = text.find(KEY)? + KEY.len();
    let end = text[start..].find('"')? + start;
    Some(PathBuf::from(&text[start..end]))
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn player_name() -> &'static str {
    if cfg!(windows) {
        "eme_player.exe"
    } else {
        "eme_player"
    }
}

fn say(tx: &Sender<Msg>, level: Level, text: impl Into<String>) {
    let _ = tx.send(Msg::Line(level, text.into()));
}

fn io(path: &Path, err: std::io::Error) -> String {
    format!("{}: {err}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eme-build-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn lines(rx: &Receiver<Msg>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|msg| match msg {
                Msg::Line(_, text) => Some(text),
                Msg::Done(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_selected_profile_chooses_its_player_binary() {
        let dir = scratch("profiles");
        write(&dir.join("debug").join(player_name()), b"debug");
        write(&dir.join("release").join(player_name()), b"release");
        let (tx, _rx) = mpsc::channel();
        assert_eq!(
            resolve_player(&dir, Profile::Release, &tx).unwrap(),
            dir.join("release").join(player_name())
        );
        assert_eq!(
            resolve_player(&dir, Profile::Debug, &tx).unwrap(),
            dir.join("debug").join(player_name())
        );
    }

    #[test]
    fn stage_writes_the_game_layout() {
        let dir = scratch("stage");
        let player = dir.join("player");
        write(&player, b"#!/bin/true\n");
        let scene = dir.join("main.ember");
        write(&scene, b"(format_version:1, objects:[])");
        write(&dir.join("assets/textures/a.txt"), b"a");
        let assets = dir.join("assets");
        let out = dir.join("build/linux");

        let (tx, rx) = mpsc::channel();
        stage(&player, &scene, &out, Some(&assets), &tx).unwrap();
        drop(tx);

        assert!(out.join(GAME_BIN).is_file());
        assert!(out.join(SCENE_FILE).is_file());
        assert!(out.join("assets/textures/a.txt").is_file());
        let log = lines(&rx);
        assert!(log.iter().any(|line| line.contains(OUT_DIR)), "{log:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn staged_player_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("exec");
        let player = dir.join("player");
        write(&player, b"#!/bin/true\n");
        std::fs::set_permissions(&player, std::fs::Permissions::from_mode(0o644)).unwrap();
        let scene = dir.join("main.ember");
        write(&scene, b"x");
        let out = dir.join("build/linux");

        let (tx, _rx) = mpsc::channel();
        stage(&player, &scene, &out, None, &tx).unwrap();
        let mode = std::fs::metadata(out.join(GAME_BIN))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stage_cleans_a_previous_build() {
        let dir = scratch("clean");
        let player = dir.join("player");
        write(&player, b"bin");
        let scene = dir.join("main.ember");
        write(&scene, b"x");
        let out = dir.join("build/linux");
        write(&out.join("stale.txt"), b"old");

        let (tx, _rx) = mpsc::channel();
        stage(&player, &scene, &out, None, &tx).unwrap();
        assert!(!out.join("stale.txt").exists());
        assert!(out.join(GAME_BIN).is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn player_prefers_release_over_debug() {
        let [first, second] = candidates(Path::new("target"));
        assert_eq!(first, Path::new("target/release").join(player_name()));
        assert_eq!(second, Path::new("target/debug").join(player_name()));
    }

    #[test]
    fn targets_name_every_platform() {
        assert_eq!(Target::ALL.len(), 3);
        for target in Target::ALL {
            assert!(!target.label().is_empty());
        }
        assert!(Target::Linux.ready());
        assert!(!Target::Windows.ready());
        assert!(!Target::MacOs.ready());
    }

    #[test]
    fn log_keeps_the_tail() {
        let mut state = BuildState::default();
        for i in 0..MAX_LINES + 7 {
            state.line(Level::Step, i.to_string());
        }
        assert_eq!(state.log.len(), MAX_LINES);
        assert_eq!(state.log.last().unwrap().text, (MAX_LINES + 6).to_string());
    }
}
