//! Builds the C# project behind a script assembly without leaving the editor.
//!
//! The dialog follows the same shape as [`crate::build`]: a worker thread runs
//! `dotnet build`, the receiver drains its lines into the console, and success
//! is left to [`crate::script_watch`] to notice and reload.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use eme::prelude::*;

use crate::console::ConsoleLog;

/// Folder under the workspace holding the C# game projects.
pub const PROJECT_ROOT: &str = "sdk/csharp";

/// Result of the last build this editor started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Assembly path relative to the asset root.
    pub assembly: String,
    pub ok: bool,
}

enum Msg {
    Line(String),
    Done(bool),
}

/// Whether a C# build is running, and how the last one ended.
#[derive(Resource, Default)]
pub struct CSharpBuild {
    running: bool,
    assembly: String,
    last: Option<Outcome>,
    rev: u64,
    rx: Option<Mutex<Receiver<Msg>>>,
}

impl CSharpBuild {
    /// Bumped when the visible state changes.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn last(&self) -> Option<&Outcome> {
        self.last.as_ref()
    }

    /// Starts `dotnet build` for `assembly`, writing next to it under `assets`.
    ///
    /// The project is found by the assembly's file stem: `Game.dll` builds
    /// `sdk/csharp/Game/Game.csproj`. A missing project is recorded as a failed
    /// outcome and returned so the caller can log it.
    pub fn start(&mut self, workspace: &Path, assets: &Path, assembly: &str) -> Result<(), String> {
        if self.running {
            return Err("a C# build is already running".into());
        }
        let Some(project) = project_for(workspace, assembly) else {
            let wanted = expected_project(workspace, assembly)
                .map(|path| relative(&path, workspace))
                .unwrap_or_else(|| "sdk/csharp/<Assembly>/<Assembly>.csproj".into());
            let err = format!("no project at {wanted}");
            self.failure(assembly);
            return Err(err);
        };
        let out = assets
            .join(assembly)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| assets.to_path_buf());
        let (tx, rx) = mpsc::channel();
        self.running = true;
        self.assembly = assembly.to_owned();
        self.rx = Some(Mutex::new(rx));
        self.rev += 1;
        let assembly = assembly.to_owned();
        thread::spawn(move || run(project, out, assembly, tx));
        Ok(())
    }

    fn failure(&mut self, assembly: &str) {
        self.running = false;
        self.rx = None;
        self.last = Some(Outcome {
            assembly: assembly.to_owned(),
            ok: false,
        });
        self.rev += 1;
    }
}

/// Assembly file stem, e.g. `Game` for `scripts/managed/Game.dll`.
pub fn assembly_stem(assembly: &str) -> Option<&str> {
    let name = Path::new(assembly).file_name()?.to_str()?;
    let stem = name
        .strip_suffix(".dll")
        .or_else(|| name.strip_suffix(".DLL"))?;
    (!stem.is_empty()).then_some(stem)
}

/// Project the naming convention points at, whether or not it exists.
pub fn expected_project(workspace: &Path, assembly: &str) -> Option<PathBuf> {
    let stem = assembly_stem(assembly)?;
    Some(
        workspace
            .join(PROJECT_ROOT)
            .join(stem)
            .join(format!("{stem}.csproj")),
    )
}

/// Project that owns `assembly`, falling back to a scan of [`PROJECT_ROOT`].
pub fn project_for(workspace: &Path, assembly: &str) -> Option<PathBuf> {
    let expected = expected_project(workspace, assembly)?;
    if expected.is_file() {
        return Some(expected);
    }
    let stem = assembly_stem(assembly)?;
    let want = format!("{stem}.csproj");
    let entries = std::fs::read_dir(workspace.join(PROJECT_ROOT)).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path().join(&want))
        .find(|candidate| candidate.is_file())
}

fn relative(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn run(project: PathBuf, out: PathBuf, assembly: String, tx: Sender<Msg>) {
    let mut cmd = Command::new("dotnet");
    cmd.arg("build")
        .arg(&project)
        .arg("-c")
        .arg("Release")
        .arg("-o")
        .arg(&out)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = tx.send(Msg::Line(format!("csharp: dotnet build: {err}")));
            let _ = tx.send(Msg::Done(false));
            return;
        }
    };
    let stderr = child.stderr.take();
    let err_tx = tx.clone();
    let reader = thread::spawn(move || {
        let Some(stderr) = stderr else {
            return;
        };
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = err_tx.send(Msg::Line(line));
        }
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(Msg::Line(line));
        }
    }
    let ok = child.wait().is_ok_and(|status| status.success());
    let _ = reader.join();
    let summary = if ok {
        format!("csharp: built {assembly}")
    } else {
        format!("csharp: build failed for {assembly}")
    };
    let _ = tx.send(Msg::Line(summary));
    let _ = tx.send(Msg::Done(ok));
}

/// Drains the worker's output into the console and the inspector status.
pub fn poll_csharp_build(mut build: ResMut<CSharpBuild>, mut log: ResMut<ConsoleLog>) {
    let msgs = {
        let Some(rx) = build.rx.as_ref() else {
            return;
        };
        let Ok(rx) = rx.lock() else {
            return;
        };
        rx.try_iter().collect::<Vec<Msg>>()
    };
    for msg in msgs {
        match msg {
            Msg::Line(text) => {
                if !text.trim().is_empty() {
                    log.push(text);
                }
            }
            Msg::Done(ok) => {
                build.running = false;
                build.rx = None;
                let assembly = build.assembly.clone();
                build.last = Some(Outcome { assembly, ok });
                build.rev += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fabr-script-build-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn dotnet_available() -> bool {
        Command::new("dotnet")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn the_sample_project_resolves_from_the_assembly_name() {
        let project = project_for(&workspace(), "scripts/managed/Game.dll").unwrap();
        assert!(
            project.ends_with("sdk/csharp/Game/Game.csproj"),
            "{}",
            project.display()
        );
    }

    #[test]
    fn a_project_outside_the_named_folder_still_resolves() {
        let dir = scratch("scan");
        let project = dir.join("sdk/csharp/Other/Game.csproj");
        std::fs::create_dir_all(project.parent().unwrap()).unwrap();
        std::fs::write(&project, "<Project />").unwrap();
        assert_eq!(
            project_for(&dir, "scripts/managed/Game.dll").unwrap(),
            project
        );
    }

    #[test]
    fn only_assembly_names_resolve() {
        let dir = scratch("names");
        assert!(project_for(&dir, "scripts/managed/Game.lua").is_none());
        assert!(project_for(&dir, "scripts/managed/Game").is_none());
        assert!(project_for(&dir, "scripts/managed/.dll").is_none());
        assert!(project_for(&dir, "").is_none());
    }

    #[test]
    fn a_missing_project_is_recorded_as_a_failed_build() {
        let dir = scratch("missing");
        let assets = dir.join("assets");
        let mut build = CSharpBuild::default();
        let err = build
            .start(&dir, &assets, "scripts/managed/Game.dll")
            .unwrap_err();
        assert!(err.contains("sdk/csharp/Game/Game.csproj"), "{err}");
        assert_eq!(
            build.last(),
            Some(&Outcome {
                assembly: "scripts/managed/Game.dll".into(),
                ok: false,
            })
        );
        assert!(!build.running());
    }

    #[test]
    fn the_sample_project_builds_into_the_script_folder() {
        if !dotnet_available() {
            return;
        }
        let assets = scratch("build");
        let mut build = CSharpBuild::default();
        build
            .start(&workspace(), &assets, "scripts/managed/Game.dll")
            .unwrap();
        let rx = build.rx.as_ref().unwrap().lock().unwrap();
        let msgs: Vec<Msg> = rx.iter().collect();
        assert!(
            msgs.iter().any(|msg| matches!(msg, Msg::Done(true))),
            "build did not succeed"
        );
        assert!(assets.join("scripts/managed/Game.dll").is_file());
    }
}
