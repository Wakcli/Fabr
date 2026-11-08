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
    /// Native Linux folder.
    #[default]
    Linux,
    /// Windows executable cross-compiled with the MinGW toolchain.
    Windows,
    /// Needs a macOS host: Apple's linker cannot run anywhere else.
    MacOs,
}

/// Rust target triple the Windows export is compiled for.
pub const WINDOWS_TRIPLE: &str = "x86_64-pc-windows-gnu";
/// MinGW GNU C compiler used as the Windows linker.
pub const MINGW_LINKER: &str = "x86_64-w64-mingw32-gcc";

impl Target {
    pub const ALL: [Target; 3] = [Target::Linux, Target::Windows, Target::MacOs];

    pub fn label(self) -> &'static str {
        match self {
            Target::Linux => "Linux",
            Target::Windows => "Windows",
            Target::MacOs => "macOS",
        }
    }

    /// Folder the staged game is written to.
    pub fn out_dir(self) -> &'static str {
        match self {
            Target::Linux => "build/linux",
            Target::Windows => "build/windows",
            Target::MacOs => "build/macos",
        }
    }

    /// Name the player binary takes in the game folder.
    pub fn binary_name(self) -> &'static str {
        match self {
            Target::Windows => "emberrly-game.exe",
            _ => "emberrly-game",
        }
    }

    /// Cargo `--target` triple, or `None` for the host.
    pub fn cargo_target(self) -> Option<&'static str> {
        match self {
            Target::Linux => None,
            Target::Windows => Some(WINDOWS_TRIPLE),
            Target::MacOs => Some("x86_64-apple-darwin"),
        }
    }

    /// Name cargo gives the player binary for this target.
    pub fn player_name(self) -> &'static str {
        match self {
            Target::Windows => "eme_player.exe",
            _ => "eme_player",
        }
    }

    /// Whether this machine can assemble the target end to end.
    pub fn ready(self) -> bool {
        match self {
            Target::Linux => true,
            Target::Windows => {
                mingw_linker().is_some() && toolchain_for(WINDOWS_TRIPLE, toolchain_root()).is_some()
            }
            Target::MacOs => false,
        }
    }

    /// One-line explanation shown under the target chips.
    pub fn note(self) -> &'static str {
        match self {
            Target::Linux => "Native build, staged for the current machine.",
            Target::Windows => {
                "Cross-compiled with MinGW; needs the Rust Windows target installed."
            }
            Target::MacOs => "Needs a macOS host or CI runner with Apple's linker.",
        }
    }
}

/// Cargo invocation plus the environment a cross build needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoCommand {
    /// Cargo executable.
    pub program: PathBuf,
    /// `rustc` to force, when a specific toolchain was picked.
    pub rustc: Option<PathBuf>,
    /// Human-readable toolchain label for the build log.
    pub label: String,
}

/// Picks cargo for `target`, preferring a toolchain that has the target std.
pub fn cargo_command(target: Target) -> CargoCommand {
    let Some(triple) = target.cargo_target() else {
        return CargoCommand {
            program: PathBuf::from("cargo"),
            rustc: None,
            label: "host cargo".into(),
        };
    };
    if let Some(program) = std::env::var_os("FABR_CARGO") {
        let program = PathBuf::from(program);
        return CargoCommand {
            label: format!("{} (FABR_CARGO)", program.display()),
            rustc: program.parent().map(|dir| dir.join("rustc")).filter(|p| p.is_file()),
            program,
        };
    }
    if let Some(toolchain) = toolchain_for(triple, toolchain_root()) {
        return CargoCommand {
            program: toolchain.cargo,
            rustc: toolchain.rustc,
            label: toolchain.label,
        };
    }
    CargoCommand {
        program: PathBuf::from("cargo"),
        rustc: None,
        label: format!("host cargo (no {triple} std found)"),
    }
}

/// One Rust toolchain that carries a target's standard library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    /// `bin/cargo` inside the toolchain.
    pub cargo: PathBuf,
    /// `bin/rustc` inside the toolchain.
    pub rustc: Option<PathBuf>,
    /// Toolchain name, used in the build log.
    pub label: String,
}

/// Folder holding rustup toolchains.
pub fn toolchain_root() -> PathBuf {
    if let Some(home) = std::env::var_os("RUSTUP_HOME") {
        return PathBuf::from(home).join("toolchains");
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".rustup/toolchains"))
        .unwrap_or_else(|| PathBuf::from(".rustup/toolchains"))
}

/// Finds the newest toolchain under `root` with `triple`'s std installed.
pub fn toolchain_for(triple: &str, root: PathBuf) -> Option<Toolchain> {
    let entries = std::fs::read_dir(&root).ok()?;
    let mut best: Option<Toolchain> = None;
    for entry in entries.flatten() {
        let dir = entry.path();
        let cargo = dir.join("bin/cargo");
        let rustc = dir.join("bin/rustc");
        if !cargo.is_file() || !dir.join("lib/rustlib").join(triple).join("lib").is_dir() {
            continue;
        }
        let label = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let candidate = Toolchain {
            cargo,
            rustc: rustc.is_file().then_some(rustc),
            label,
        };
        if best
            .as_ref()
            .is_none_or(|current| candidate.label > current.label)
        {
            best = Some(candidate);
        }
    }
    best
}

fn mingw_linker() -> Option<&'static Path> {
    static LINKER: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    LINKER
        .get_or_init(|| {
            let path = std::env::var_os("PATH")?;
            std::env::split_paths(&path)
                .map(|dir| dir.join(MINGW_LINKER))
                .find(|candidate| candidate.is_file())
        })
        .as_deref()
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

    /// Cargo's output subfolder for this profile.
    pub fn dir_name(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Debug => "debug",
        }
    }

    fn cargo_flags(self) -> &'static [&'static str] {
        match self {
            Profile::Release => &["--release"],
            Profile::Debug => &[],
        }
    }
}

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
    /// Platform the next build produces.
    pub target: Target,
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

    /// Selects the build target.
    pub fn set_target(&mut self, target: Target) {
        if self.target == target || !target.ready() {
            return;
        }
        self.target = target;
        self.rev += 1;
    }

    /// Starts a background build of `doc`; ignored while one is running.
    pub fn start(&mut self, doc: SceneDoc) {
        if self.running {
            return;
        }
        if !self.target.ready() {
            self.line(
                Level::Error,
                format!(
                    "{} needs a toolchain this machine does not have",
                    self.target.label()
                ),
            );
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
        let target = self.target;
        thread::spawn(move || run(doc, profile, target, tx));
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

fn run(doc: SceneDoc, profile: Profile, target: Target, tx: Sender<Msg>) {
    match package(&doc, profile, target, &tx) {
        Ok(dir) => {
            let abs = std::fs::canonicalize(&dir).unwrap_or(dir);
            say(&tx, Level::Done, format!("game folder {}", abs.display()));
            let _ = tx.send(Msg::Done(Ok(abs.join(target.binary_name()))));
        }
        Err(err) => {
            say(&tx, Level::Error, format!("build failed: {err}"));
            let _ = tx.send(Msg::Done(Err(err)));
        }
    }
}

fn package(
    doc: &SceneDoc,
    profile: Profile,
    target: Target,
    tx: &Sender<Msg>,
) -> Result<PathBuf, String> {
    say(tx, Level::Step, format!("target {}", target.label()));
    let scene = persist::save_to(doc, persist::SCENE_PATH)
        .map_err(|err| format!("could not save scene: {err}"))?;
    say(tx, Level::Step, format!("saved scene {}", scene.display()));

    let out_dir = target_dir();
    say(tx, Level::Step, format!("target dir {}", out_dir.display()));
    let export_dir = out_dir.join("export");
    let player = resolve_player(&export_dir, profile, target, tx)?;

    let out = PathBuf::from(target.out_dir());
    let assets = Path::new(ASSET_DIR);
    stage(
        &player,
        &scene,
        &out,
        assets.is_dir().then_some(assets),
        target,
        tx,
    )?;
    Ok(out)
}

/// Picks the player binary, building it first when cargo has not produced one.
fn resolve_player(
    export_dir: &Path,
    profile: Profile,
    target: Target,
    tx: &Sender<Msg>,
) -> Result<PathBuf, String> {
    let chosen = player_path(export_dir, target, profile);
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
            "no player {} binary, running cargo build -p {PLAYER_PKG} {}{}",
            profile.label().to_ascii_lowercase(),
            profile.cargo_flags().join(" "),
            target
                .cargo_target()
                .map(|triple| format!(" --target {triple}"))
                .unwrap_or_default()
        ),
    );
    build_player(tx, profile, target, export_dir)?;
    if chosen.is_file() {
        say(tx, Level::Step, "player built");
        return Ok(chosen);
    }
    Err(format!(
        "cargo finished but {} is missing",
        chosen.display()
    ))
}

/// Where cargo puts the player for `target` and `profile`.
pub fn player_path(target_dir: &Path, target: Target, profile: Profile) -> PathBuf {
    let root = match target.cargo_target() {
        Some(triple) => target_dir.join(triple),
        None => target_dir.to_path_buf(),
    };
    root.join(profile.dir_name()).join(target.player_name())
}

/// Player locations in preference order: release first, then debug.
fn candidates(target: &Path) -> [PathBuf; 2] {
    [
        target.join("release").join(Target::Linux.player_name()),
        target.join("debug").join(Target::Linux.player_name()),
    ]
}

fn build_player(
    tx: &Sender<Msg>,
    profile: Profile,
    target: Target,
    export_dir: &Path,
) -> Result<(), String> {
    let cargo = cargo_command(target);
    say(tx, Level::Step, format!("toolchain {}", cargo.label));
    say(
        tx,
        Level::Step,
        "static player: external plugins load in the editor and dev player only",
    );
    let mut command = Command::new(&cargo.program);
    command
        .args(["build", "-p", PLAYER_PKG, "--no-default-features"])
        .args(profile.cargo_flags())
        .env("CARGO_TARGET_DIR", export_dir);
    if let Some(triple) = target.cargo_target() {
        command.args(["--target", triple]);
        if triple == WINDOWS_TRIPLE {
            let linker = mingw_linker().ok_or_else(|| {
                format!("{MINGW_LINKER} is not installed, so Windows cannot be linked")
            })?;
            command.env(
                "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER",
                linker.as_os_str(),
            );
            say(tx, Level::Step, format!("linker {}", linker.display()));
        }
    }
    if let Some(rustc) = &cargo.rustc {
        command.env("RUSTC", rustc);
    }
    let mut child = command
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
    target: Target,
    tx: &Sender<Msg>,
) -> Result<(), String> {
    if out.exists() {
        std::fs::remove_dir_all(out).map_err(|err| io(out, err))?;
        say(tx, Level::Step, format!("cleaned {}", target.out_dir()));
    }
    std::fs::create_dir_all(out).map_err(|err| io(out, err))?;

    let game = out.join(target.binary_name());
    std::fs::copy(player, &game).map_err(|err| io(player, err))?;
    set_executable(&game)?;
    say(
        tx,
        Level::Step,
        format!("copied player to {}/{}", target.out_dir(), target.binary_name()),
    );

    std::fs::copy(scene, out.join(SCENE_FILE)).map_err(|err| io(scene, err))?;
    say(
        tx,
        Level::Step,
        format!("copied scene to {}/{SCENE_FILE}", target.out_dir()),
    );

    match assets {
        Some(src) => {
            copy_dir(src, &out.join(ASSET_DIR))?;
            say(tx, Level::Step, format!("copied {ASSET_DIR}/"));
        }
        None => say(tx, Level::Step, "no assets/ folder to copy"),
    }
    stage_packages(&workspace_dir(), out, tx)?;

    let settings = workspace_dir().join(eme_editor_project::SETTINGS_FILE);
    if settings.is_file() {
        std::fs::copy(&settings, out.join(eme_editor_project::SETTINGS_FILE))
            .map_err(|err| io(&settings, err))?;
        say(tx, Level::Step, "copied project settings");
    }
    Ok(())
}

/// Copies every installed package's assets into the staged game.
fn stage_packages(root: &Path, out: &Path, tx: &Sender<Msg>) -> Result<(), String> {
    let set = eme_editor_project::PackageSet::load(&root.join(eme_editor_project::PACKAGES_FILE))?;
    if set.packages.is_empty() {
        say(tx, Level::Step, "no packages to stage");
        return Ok(());
    }
    let lock = set.resolve(root)?;
    lock.save(&root.join(eme_editor_project::LOCK_FILE))?;
    say(
        tx,
        Level::Step,
        format!("resolved {} packages", lock.packages.len()),
    );
    for entry in &lock.packages {
        let manifest = eme_editor_project::PackageSet::manifest(
            root,
            &eme_editor_project::PackageEntry {
                path: entry.path.clone(),
            },
        )?;
        let src = root.join(&entry.path).join(&manifest.assets);
        if src.is_dir() {
            copy_dir(&src, &out.join(ASSET_DIR))?;
        }
        say(
            tx,
            Level::Step,
            format!(
                "staged package {} {} from {}",
                manifest.name, entry.version, entry.path
            ),
        );
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
        let name = Target::Linux.player_name();
        write(&dir.join("debug").join(name), b"debug");
        write(&dir.join("release").join(name), b"release");
        let (tx, _rx) = mpsc::channel();
        assert_eq!(
            resolve_player(&dir, Profile::Release, Target::Linux, &tx).unwrap(),
            dir.join("release").join(name)
        );
        assert_eq!(
            resolve_player(&dir, Profile::Debug, Target::Linux, &tx).unwrap(),
            dir.join("debug").join(name)
        );
    }

    #[test]
    fn each_target_stages_its_own_layout() {
        assert_eq!(Target::Linux.out_dir(), "build/linux");
        assert_eq!(Target::Linux.binary_name(), "emberrly-game");
        assert_eq!(Target::Windows.out_dir(), "build/windows");
        assert_eq!(Target::Windows.binary_name(), "emberrly-game.exe");
        assert_eq!(Target::Windows.cargo_target(), Some("x86_64-pc-windows-gnu"));
        assert_eq!(Target::Linux.cargo_target(), None);
        assert!(!Target::MacOs.ready());
    }

    #[test]
    fn cross_player_paths_include_the_target_directory() {
        let dir = Path::new("/target");
        assert_eq!(
            player_path(dir, Target::Windows, Profile::Release),
            Path::new("/target/x86_64-pc-windows-gnu/release/eme_player.exe")
        );
        assert_eq!(
            player_path(dir, Target::Linux, Profile::Debug),
            Path::new("/target/debug/eme_player")
        );
    }

    #[test]
    fn toolchains_with_the_target_std_are_preferred_newest_first() {
        let root = scratch("toolchains");
        let with_std = root.join("1.90.0/bin");
        write(&with_std.join("cargo"), b"");
        write(&with_std.join("rustc"), b"");
        write(
            &root.join("1.90.0/lib/rustlib/x86_64-pc-windows-gnu/lib/libcore.rlib"),
            b"",
        );
        let newer = root.join("1.99.0/bin");
        write(&newer.join("cargo"), b"");
        write(&newer.join("rustc"), b"");
        write(
            &root.join("1.99.0/lib/rustlib/x86_64-pc-windows-gnu/lib/libcore.rlib"),
            b"",
        );
        write(&root.join("1.80.0/bin/cargo"), b"");

        let picked = toolchain_for("x86_64-pc-windows-gnu", root.clone()).unwrap();
        assert_eq!(picked.cargo, newer.join("cargo"));
        assert_eq!(picked.rustc, Some(newer.join("rustc")));
        assert_eq!(picked.label, "1.99.0");
        assert!(toolchain_for("aarch64-apple-darwin", root).is_none());
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
        stage(
            &player,
            &scene,
            &out,
            Some(&assets),
            Target::Linux,
            &tx,
        )
        .unwrap();
        drop(tx);

        assert!(out.join(Target::Linux.binary_name()).is_file());
        assert!(out.join(SCENE_FILE).is_file());
        assert!(out.join("assets/textures/a.txt").is_file());
        let log = lines(&rx);
        assert!(
            log.iter().any(|line| line.contains(Target::Linux.out_dir())),
            "{log:?}"
        );
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
        stage(&player, &scene, &out, None, Target::Linux, &tx).unwrap();
        let mode = std::fs::metadata(out.join(Target::Linux.binary_name()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn package_assets_are_staged_into_the_game() {
        let root = scratch("packages");
        let package = root.join("packages/sky");
        write(&package.join("fabr.package.ron"), b"(name: \"sky\", version: \"1.0.0\", assets: \"assets\")");
        write(&package.join("assets/textures/sky.png"), b"png");
        std::fs::write(
            root.join(eme_editor_project::PACKAGES_FILE),
            "(packages: [(path: \"packages/sky\")])",
        )
        .unwrap();
        let out = root.join("build/linux");
        std::fs::create_dir_all(&out).unwrap();
        let (tx, _rx) = mpsc::channel();
        stage_packages(&root, &out, &tx).unwrap();
        assert_eq!(
            std::fs::read(out.join("assets/textures/sky.png")).unwrap(),
            b"png"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn transitive_dependencies_are_staged_and_locked() {
        let root = scratch("transitive");
        let base = root.join("packages/base");
        write(&base.join("fabr.package.ron"), b"(name: \"base\", version: \"1.0.0\", assets: \"assets\")");
        write(&base.join("assets/base.txt"), b"base");
        let art = root.join("packages/art");
        write(
            &art.join("fabr.package.ron"),
            b"(name: \"art\", version: \"1.2.0\", assets: \"assets\", dependencies: [(name: \"base\", version: \"^1.0.0\")])",
        );
        write(&art.join("assets/art.txt"), b"art");
        std::fs::write(
            root.join(eme_editor_project::PACKAGES_FILE),
            "(packages: [(path: \"packages/art\")])",
        )
        .unwrap();
        let out = root.join("build/linux");
        std::fs::create_dir_all(&out).unwrap();
        let (tx, _rx) = mpsc::channel();
        stage_packages(&root, &out, &tx).unwrap();
        assert_eq!(std::fs::read(out.join("assets/art.txt")).unwrap(), b"art");
        assert_eq!(std::fs::read(out.join("assets/base.txt")).unwrap(), b"base");
        let lock = eme_editor_project::Lockfile::load(&root.join(eme_editor_project::LOCK_FILE)).unwrap();
        assert_eq!(lock.names(), ["art", "base"]);
        std::fs::remove_dir_all(&root).unwrap();
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
        stage(&player, &scene, &out, None, Target::Linux, &tx).unwrap();
        assert!(!out.join("stale.txt").exists());
        assert!(out.join(Target::Linux.binary_name()).is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn player_prefers_release_over_debug() {
        let [first, second] = candidates(Path::new("target"));
        assert_eq!(
            first,
            Path::new("target/release").join(Target::Linux.player_name())
        );
        assert_eq!(
            second,
            Path::new("target/debug").join(Target::Linux.player_name())
        );
    }

    #[test]
    fn targets_name_every_platform() {
        assert_eq!(Target::ALL.len(), 3);
        for target in Target::ALL {
            assert!(!target.label().is_empty());
        }
        assert!(Target::Linux.ready());
        assert!(!Target::MacOs.ready());
        assert_eq!(
            Target::Windows.ready(),
            mingw_linker().is_some()
                && toolchain_for(WINDOWS_TRIPLE, toolchain_root()).is_some()
        );
    }

    #[test]
    fn selecting_an_unavailable_target_is_refused() {
        let mut state = BuildState::default();
        state.set_target(Target::MacOs);
        assert_eq!(state.target, Target::Linux);
        state.set_target(Target::Linux);
        assert_eq!(state.rev, 0);
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
