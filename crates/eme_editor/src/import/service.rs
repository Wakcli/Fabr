//! Runs imports on worker threads and keeps the editor responsive.
//!
//! The UI never calls [`crate::import::import`] directly: it hands a request to
//! this service, which copies the file (when it came from outside the project),
//! parses it on a thread, and reports back through [`ImportService::poll`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Mutex;

use eme::prelude::*;

use crate::console::ConsoleLog;
use crate::import::{ImportError, ImportReport, ImportSettings, NodeDiff, Sidecar};

/// What one worker produced.
type JobResult = Result<(PathBuf, ImportReport), (PathBuf, ImportError)>;

/// A request to import one file.
#[derive(Clone, Debug)]
pub struct ImportRequest {
    /// File to read, on disk.
    pub source: PathBuf,
    /// Folder inside the project it should live in.
    pub folder: PathBuf,
    /// Options for this import.
    pub settings: ImportSettings,
    /// Copy the file into `folder` before importing.
    pub copy: bool,
}

/// Why a job is running; it decides where the finished report goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Import,
    Reload,
    Dry,
}

struct Job {
    name: String,
    kind: JobKind,
    instance: bool,
    settings: ImportSettings,
    rx: Mutex<Receiver<JobResult>>,
}

/// Background import queue and the last reports.
#[derive(Resource, Default)]
pub struct ImportService {
    jobs: Vec<Job>,
    /// What the status bar shows while a job runs.
    pub status: Option<String>,
    /// Reports keyed by the file they describe, for the inspector.
    pub reports: HashMap<PathBuf, ImportReport>,
    /// Settings used by the next plain import.
    pub settings: ImportSettings,
    /// Files that finished importing since the last drain.
    pub finished: Vec<PathBuf>,
    /// Files whose hot reimport finished, for instance refresh.
    pub reloaded: Vec<PathBuf>,
    /// Finished imports that should be placed in the scene even when the
    /// instance-after-import option is off.
    pub instantiate: Vec<PathBuf>,
    /// Dry-run outcomes keyed by source path, for the options panel.
    pub dry: HashMap<PathBuf, Result<ImportReport, String>>,
    /// Latest node-list change per asset, for the inspector.
    pub diffs: HashMap<PathBuf, NodeDiff>,
}

impl ImportService {
    /// Queues an import, copying the file into its folder first when asked.
    ///
    /// Path selection, collision handling and the rename message all happen here
    /// on the caller's thread so the log line is ordered before the work starts.
    pub fn start(&mut self, request: ImportRequest, root: &Path, log: &mut ConsoleLog) -> bool {
        self.launch(request, root, log, JobKind::Import, false)
    }

    /// Imports a file and places the finished model in the scene.
    pub fn start_in_scene(
        &mut self,
        request: ImportRequest,
        root: &Path,
        log: &mut ConsoleLog,
    ) -> bool {
        self.launch(request, root, log, JobKind::Import, true)
    }

    /// Rereads an already imported model after its file changed on disk.
    pub fn reload(&mut self, source: &Path, log: &mut ConsoleLog) -> bool {
        self.launch(
            ImportRequest {
                source: source.to_path_buf(),
                folder: PathBuf::new(),
                settings: ImportSettings::default(),
                copy: false,
            },
            Path::new(""),
            log,
            JobKind::Reload,
            false,
        )
    }

    /// Parses a model without writing a sidecar, for the options panel.
    pub fn dry_run(
        &mut self,
        source: &Path,
        settings: ImportSettings,
        log: &mut ConsoleLog,
    ) -> bool {
        self.launch(
            ImportRequest {
                source: source.to_path_buf(),
                folder: PathBuf::new(),
                settings,
                copy: false,
            },
            Path::new(""),
            log,
            JobKind::Dry,
            false,
        )
    }

    fn launch(
        &mut self,
        request: ImportRequest,
        root: &Path,
        log: &mut ConsoleLog,
        kind: JobKind,
        instance: bool,
    ) -> bool {
        let ImportRequest {
            source,
            folder,
            settings,
            copy,
        } = request;

        let dest = if copy {
            let target_dir = root.join(&folder);
            if let Err(err) = std::fs::create_dir_all(&target_dir) {
                log.push(format!("import failed: {}: {err}", target_dir.display()));
                return false;
            }
            let Some((dest, renamed)) = unique_destination(&target_dir, &source) else {
                log.push(format!(
                    "import failed: no free name for {} in {}",
                    source.display(),
                    target_dir.display()
                ));
                return false;
            };
            if renamed {
                log.push(format!(
                    "{} already existed; importing as {}",
                    source.file_name().unwrap_or_default().to_string_lossy(),
                    dest.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
            if let Err(err) = std::fs::copy(&source, &dest) {
                log.push(format!("import failed: {}: {err}", dest.display()));
                return false;
            }
            dest
        } else {
            source
        };

        let name = dest
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| dest.display().to_string());
        // a stored sidecar is the authoring state; a reimport must honour it,
        // while a dry run shows the options the panel currently has
        let settings = if kind == JobKind::Dry {
            settings
        } else {
            Sidecar::load(&dest).map_or(settings, |sidecar| sidecar.settings)
        };
        self.status = Some(format!("importing {name}…"));

        let (tx, rx) = channel();
        let worker_dest = dest.clone();
        let worker_settings = settings.clone();
        std::thread::spawn(move || {
            let result = crate::import::import(&worker_dest, &worker_settings)
                .map(|report| (worker_dest.clone(), report))
                .map_err(|err| (worker_dest.clone(), err));
            let _ = tx.send(result);
        });

        if kind != JobKind::Dry {
            self.settings = settings.clone();
        }
        self.jobs.push(Job {
            name,
            kind,
            instance,
            settings,
            rx: Mutex::new(rx),
        });
        true
    }

    /// Collects finished jobs; returns true when anything changed.
    pub fn poll(&mut self, log: &mut ConsoleLog) -> bool {
        let mut finished = Vec::new();
        let mut changed = false;

        for (index, job) in self.jobs.iter().enumerate() {
            let outcome = match job.rx.lock() {
                Ok(rx) => match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Err((
                        PathBuf::new(),
                        ImportError::Io(format!("{}: worker stopped", job.name)),
                    ))),
                },
                Err(_) => None,
            };
            if let Some(result) = outcome {
                let kind = job.kind;
                let instance = job.instance;
                finished.push(index);
                changed = true;
                match result {
                    Ok((path, report)) => {
                        let warns = report.warnings.len();
                        log.push(report.summary());
                        if warns > 0 {
                            for warning in &report.warnings {
                                let target = warning
                                    .target
                                    .as_deref()
                                    .map_or_else(String::new, |target| format!(" [{target}]"));
                                log.push(format!("  {}: {}{target}", warning.severity, warning.message));
                            }
                        }
                        match kind {
                            JobKind::Dry => {
                                self.dry.insert(path, Ok(report));
                            }
                            JobKind::Import | JobKind::Reload => {
                                let keys: Vec<eme_game_scene::NodeKey> =
                                    report.nodes.iter().map(|node| node.key.clone()).collect();
                                let diff = crate::import::diff::update(&path, &keys);
                                self.reports.insert(path.clone(), report);
                                if kind == JobKind::Import {
                                    self.finished.push(path.clone());
                                    if instance {
                                        self.instantiate.push(path.clone());
                                    }
                                } else {
                                    self.reloaded.push(path.clone());
                                }
                                let sidecar = Sidecar {
                                    version: crate::import::sidecar::SIDECAR_VERSION,
                                    guid: Sidecar::load_or_new(&path).guid,
                                    settings: self
                                        .jobs
                                        .get(index)
                                        .map_or_else(ImportSettings::default, |job| {
                                            job.settings.clone()
                                        }),
                                };
                                if let Err(err) = sidecar.save(&path) {
                                    log.push(format!("could not save sidecar: {err}"));
                                }
                                for line in diff.summary_lines() {
                                    log.push(line);
                                }
                                self.diffs.insert(path.clone(), diff);
                            }
                        }
                    }
                    Err((path, err)) => {
                        if kind == JobKind::Dry {
                            log.push(format!("dry run failed: {err}"));
                            self.dry.insert(path, Err(err.to_string()));
                        } else {
                            log.push(format!("import failed: {err}"));
                        }
                    }
                }
            }
        }

        for index in finished.into_iter().rev() {
            self.jobs.remove(index);
            if self.jobs.is_empty() {
                self.status = None;
            }
        }
        if !self.jobs.is_empty() {
            let running = self.jobs.len();
            self.status = Some(format!("importing {running} file(s)…"));
        }
        changed
    }

    /// The report for `path`, if it was imported this session.
    pub fn report(&self, path: &Path) -> Option<&ImportReport> {
        self.reports.get(path)
    }
}

/// The status-bar text that shows what is importing.
#[derive(Component, Clone, Copy, Default)]
pub struct ImportStatus;

/// Drains finished imports into the console and keeps the GUID index current.
pub fn poll_system(
    mut service: ResMut<ImportService>,
    mut index: ResMut<crate::scene::ModelIndex>,
    mut log: ResMut<ConsoleLog>,
) {
    if service.poll(&mut log) {
        for path in service.finished.iter().chain(service.reloaded.iter()) {
            index.0.upsert(path);
        }
    }
}

/// Mirrors the service status into the status bar.
pub fn status_system(service: Res<ImportService>, mut texts: Query<&mut Text, With<ImportStatus>>) {
    let want = service.status.clone().unwrap_or_default();
    for mut text in &mut texts {
        if text.0 != want {
            text.0 = want.clone();
        }
    }
}

/// Picks a destination that does not exist yet, adding ` (n)` when it does.
///
/// Returns `None` when a thousand names are taken, which means something is
/// wrong with the folder rather than with the file.
pub fn unique_destination(folder: &Path, source: &Path) -> Option<(PathBuf, bool)> {
    let name = source.file_name()?;
    let direct = folder.join(name);
    if !direct.exists() {
        return Some((direct, false));
    }
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let extension = source
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    for index in 1..1000 {
        let candidate = folder.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return Some((candidate, true));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fabr-service-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_free_name_is_used_unchanged() {
        let dir = scratch("free");
        let (dest, renamed) = unique_destination(&dir, Path::new("/models/chair.glb")).unwrap();
        assert_eq!(dest, dir.join("chair.glb"));
        assert!(!renamed);
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let dir = scratch("collision");
        std::fs::write(dir.join("chair.glb"), b"first").unwrap();

        let (dest, renamed) = unique_destination(&dir, Path::new("/models/chair.glb")).unwrap();
        assert_eq!(dest, dir.join("chair (1).glb"));
        assert!(renamed);
        assert_eq!(std::fs::read(dir.join("chair.glb")).unwrap(), b"first");

        std::fs::write(&dest, b"second").unwrap();
        let (third, _) = unique_destination(&dir, Path::new("/models/chair.glb")).unwrap();
        assert_eq!(third, dir.join("chair (2).glb"));
    }

    const TRI_GLTF: &str = r#"{"asset":{"version":"2.0","generator":"service-test"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"tri","mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"buffers":[{"byteLength":42,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAAAAABAAIA"}]}"#;

    /// Drains the service until a condition holds, without sleeping.
    fn settle(service: &mut ImportService, log: &mut ConsoleLog, done: impl Fn(&ImportService) -> bool) {
        for _ in 0..20_000 {
            service.poll(log);
            if done(service) {
                return;
            }
            std::thread::yield_now();
        }
        panic!("import did not finish");
    }

    #[test]
    fn a_dropped_file_is_copied_and_imported() {
        let root = scratch("e2e-root");
        let outside = scratch("e2e-src");
        let source = outside.join("tri.gltf");
        std::fs::write(&source, TRI_GLTF).unwrap();

        let mut log = ConsoleLog::default();
        let mut service = ImportService::default();
        assert!(service.start(
            ImportRequest {
                source,
                folder: PathBuf::from("assets"),
                settings: ImportSettings::default(),
                copy: true,
            },
            &root,
            &mut log,
        ));

        let copied = root.join("assets").join("tri.gltf");
        settle(&mut service, &mut log, |_| copied.exists());
        assert!(copied.exists(), "the file must be copied into the project");
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), TRI_GLTF);

        settle(&mut service, &mut log, |service| !service.reports.is_empty());
        let report = service.report(&copied).expect("report is stored");
        assert_eq!(report.counts.nodes, 1);
        assert!(
            log.lines().iter().any(|line| line.starts_with("imported tri.gltf")),
            "the console gets the summary line: {:?}",
            log.lines()
        );
        assert!(service.status.is_none(), "status clears when the queue drains");
    }

    #[test]
    fn a_broken_file_becomes_a_console_error() {
        let root = scratch("broken-root");
        let outside = scratch("broken-src");
        let source = outside.join("broken.gltf");
        std::fs::write(&source, b"not a model").unwrap();

        let mut log = ConsoleLog::default();
        let mut service = ImportService::default();
        assert!(service.start(
            ImportRequest {
                source,
                folder: PathBuf::from("assets"),
                settings: ImportSettings::default(),
                copy: true,
            },
            &root,
            &mut log,
        ));

        settle(&mut service, &mut log, |service| service.status.is_none());
        assert!(
            log.lines().iter().any(|line| line.starts_with("import failed:")),
            "bad files are logged, not panicked: {:?}",
            log.lines()
        );
    }

    #[test]
    fn a_collision_imports_under_a_new_name() {
        let root = scratch("collide-root");
        let outside = scratch("collide-src");
        let source = outside.join("chair.gltf");
        std::fs::write(&source, TRI_GLTF).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets").join("chair.gltf"), TRI_GLTF).unwrap();

        let mut log = ConsoleLog::default();
        let mut service = ImportService::default();
        assert!(service.start(
            ImportRequest {
                source,
                folder: PathBuf::from("assets"),
                settings: ImportSettings::default(),
                copy: true,
            },
            &root,
            &mut log,
        ));

        let copy = root.join("assets").join("chair (1).gltf");
        settle(&mut service, &mut log, |_| copy.exists());
        assert!(copy.exists(), "the second import lands beside the first");
        assert!(log.lines().iter().any(|line| line.contains("already existed")));
    }

    #[test]
    fn a_missing_source_reports_an_error_instead_of_panicking() {
        let dir = scratch("missing");
        let mut log = ConsoleLog::default();
        let mut service = ImportService::default();
        let started = service.start(
            ImportRequest {
                source: dir.join("nope.glb"),
                folder: PathBuf::from("assets"),
                settings: ImportSettings::default(),
                copy: true,
            },
            &dir,
            &mut log,
        );
        assert!(!started, "copying a missing file must fail");
        assert!(log.lines().last().unwrap().contains("import failed"));
        assert!(service.status.is_none());
    }

    #[test]
    fn a_failed_reload_keeps_the_old_instance() {
        use crate::scene::ModelInstance;

        let dir = scratch("failed-reload");
        let broken = dir.join("broken.glb");
        std::fs::write(&broken, b"not a model").unwrap();
        std::fs::write(
            Sidecar::path_for(&broken),
            "(version: 1, guid: \"broken-guid\", settings: ())",
        )
        .unwrap();

        let mut log = ConsoleLog::default();
        let mut service = ImportService::default();
        assert!(service.reload(&broken, &mut log));
        settle(&mut service, &mut log, |service| service.jobs.is_empty());

        let mut world = World::new();
        let old = world
            .spawn(ModelInstance {
                guid: "broken-guid".into(),
                path: "broken.glb".into(),
            })
            .id();
        assert!(service.reloaded.is_empty());
        assert!(world.get_entity(old).is_ok());
        assert!(world.get::<ModelInstance>(old).is_some());
        assert!(
            log.lines().iter().any(|line| line.contains("import failed")),
            "{:?}",
            log.lines()
        );
    }
}
