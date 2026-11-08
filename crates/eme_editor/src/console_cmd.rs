//! Console command line: parse a typed command and run it against the editor.

use eme::input::keyboard::KeyboardInput;
use eme::input_focus::{FocusCause, FocusedInput, InputFocus};
use eme::prelude::*;
use eme::text::{EditableText, TextEdit};
use eme_game_scene::ObjectKind;

use crate::console::ConsoleLog;
use crate::persist;
use crate::scene::{ModelIndex, SceneState};
use crate::undo::Undo;

/// Text field that takes console commands.
#[derive(Component, Clone, Copy, Default)]
pub struct ConsoleCommandInput;

/// Commands typed into the console, kept for arrow-key recall.
#[derive(Resource, Clone, Default)]
pub struct CommandHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
}

impl CommandHistory {
    /// Records a command, dropping exact repeats of the previous entry.
    pub fn push(&mut self, line: &str) {
        if self.entries.last().is_none_or(|last| last != line) {
            self.entries.push(line.to_owned());
        }
        self.cursor = None;
    }

    /// Previous command for Up.
    pub fn previous(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.cursor {
            None => self.entries.len() - 1,
            Some(index) => index.saturating_sub(1),
        };
        self.cursor = Some(next);
        self.entries.get(next).cloned()
    }

    /// Next command for Down.
    pub fn next(&mut self) -> Option<String> {
        let index = self.cursor?;
        if index + 1 >= self.entries.len() {
            self.cursor = None;
            return None;
        }
        self.cursor = Some(index + 1);
        self.entries.get(index + 1).cloned()
    }
}

/// Command names offered by Tab completion.
const COMMANDS: [&str; 9] = [
    "help", "clear", "save", "reload", "delete", "spawn", "scene", "select", "pkg",
];

/// Object kinds offered after `spawn`.
const KINDS: [&str; 8] = [
    "cube", "sphere", "cylinder", "plane", "empty", "sun", "pointlight", "camera",
];

/// Result of completing one console line.
#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    /// Replacement text when exactly one candidate matches.
    pub text: Option<String>,
    /// Candidates shown when the prefix is ambiguous.
    pub options: Vec<&'static str>,
}

impl Completion {
    fn unique(text: String) -> Self {
        Self {
            text: Some(text),
            options: Vec::new(),
        }
    }

    fn ambiguous(options: Vec<&'static str>) -> Self {
        Self { text: None, options }
    }
}

/// Completes the command or, after `spawn`, the object kind.
pub fn complete(input: &str) -> Completion {
    let trimmed = input.trim_start();
    let (head, tail) = match trimmed.split_once(char::is_whitespace) {
        Some((head, tail)) => (head, tail),
        None => (trimmed, ""),
    };
    if !trimmed.contains(char::is_whitespace) {
        let prefix = head.to_ascii_lowercase();
        let matches: Vec<&'static str> = COMMANDS
            .iter()
            .copied()
            .filter(|command| command.starts_with(&prefix))
            .collect();
        return match matches.as_slice() {
            [only] => Completion::unique(format!("{only} ")),
            [] => Completion::ambiguous(Vec::new()),
            many => Completion::ambiguous(many.to_vec()),
        };
    }
    if head.eq_ignore_ascii_case("spawn") {
        let prefix = tail.trim().to_ascii_lowercase();
        let matches: Vec<&'static str> = KINDS
            .iter()
            .copied()
            .filter(|kind| kind.starts_with(&prefix))
            .collect();
        return match matches.as_slice() {
            [only] => Completion::unique(format!("spawn {only}")),
            [] => Completion::ambiguous(Vec::new()),
            many => Completion::ambiguous(many.to_vec()),
        };
    }
    Completion::ambiguous(Vec::new())
}

/// One parsed console command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Print the command list.
    Help,
    /// Empty the console.
    Clear,
    /// Add a scene object.
    Spawn(ObjectKind),
    /// Delete the selection.
    Delete,
    /// Save the scene.
    Save,
    /// Ask the script runtime to reload every attached script.
    ReloadScripts,
    /// Open a scene file.
    Scene(String),
    /// Select an object by id.
    Select(u64),
    /// Install a package from a registry folder.
    PackageInstall {
        /// Registry folder, absolute or relative to the project.
        registry: String,
        /// Package name in the registry index.
        name: String,
        /// Semver requirement, for example `^1.2.0`.
        requirement: String,
    },
}

/// Parses one console line.
pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("type a command; help lists them".into());
    }
    let (name, rest) = match line.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, rest.trim()),
        None => (line, ""),
    };
    match name.to_ascii_lowercase().as_str() {
        "help" | "?" => Ok(Command::Help),
        "clear" | "cls" => Ok(Command::Clear),
        "save" => Ok(Command::Save),
        "reload" => Ok(Command::ReloadScripts),
        "delete" | "del" => Ok(Command::Delete),
        "scene" => {
            if rest.is_empty() {
                Err("scene needs a path, for example scene scenes/main.ember".into())
            } else {
                Ok(Command::Scene(rest.to_owned()))
            }
        }
        "select" => rest
            .parse()
            .map(Command::Select)
            .map_err(|_| "select needs a numeric object id".to_string()),
        "pkg" | "package" => {
            let mut parts = rest.split_whitespace();
            match parts.next() {
                Some("install") => {
                    let Some(registry) = parts.next() else {
                        return Err(
                            "pkg install needs a registry folder, for example pkg install ../registry art"
                                .into(),
                        );
                    };
                    let Some(spec) = parts.next() else {
                        return Err("pkg install needs a package name".into());
                    };
                    if parts.next().is_some() {
                        return Err("pkg install takes a registry and one package".into());
                    }
                    let (name, requirement) = match spec.split_once('@') {
                        Some((name, requirement)) if !requirement.is_empty() => {
                            (name.to_owned(), requirement.to_owned())
                        }
                        Some(_) => return Err("pkg install: empty version requirement".into()),
                        None => (spec.to_owned(), "*".to_owned()),
                    };
                    Ok(Command::PackageInstall {
                        registry: registry.to_owned(),
                        name,
                        requirement,
                    })
                }
                Some(other) => Err(format!(
                    "unknown package command {other:?}; try pkg install <registry> <name>[@<version>]"
                )),
                None => Err("pkg needs a subcommand: install <registry> <name>[@<version>]".into()),
            }
        }
        "spawn" => {
            let kind = match rest.to_ascii_lowercase().as_str() {
                "cube" => ObjectKind::Cube,
                "sphere" => ObjectKind::Sphere,
                "cylinder" => ObjectKind::Cylinder,
                "plane" => ObjectKind::Plane,
                "empty" => ObjectKind::Empty,
                "sun" => ObjectKind::Sun,
                "pointlight" | "point" => ObjectKind::PointLight,
                "camera" => ObjectKind::Camera,
                "" => return Err("spawn needs a kind: cube, sphere, cylinder, plane, empty, sun, pointlight, camera".into()),
                other => return Err(format!("unknown object kind {other:?}")),
            };
            Ok(Command::Spawn(kind))
        }
        other => Err(format!("unknown command {other:?}; try help")),
    }
}

/// Runs a parsed command and reports through the console.
pub fn execute(
    command: Command,
    state: &mut SceneState,
    undo: &mut Undo,
    log: &mut ConsoleLog,
    reload: &mut eme_script::ScriptReload,
    index: &ModelIndex,
) {
    match command {
        Command::Help => {
            for line in [
                "commands: help clear save reload delete spawn <kind> scene <path> select <id> pkg install <registry> <name>[@<version>]",
                "kinds: cube sphere cylinder plane empty sun pointlight camera",
            ] {
                log.push(line);
            }
        }
        Command::Clear => log.clear(),
        Command::Spawn(kind) => {
            undo.checkpoint(state, "Console Spawn");
            let id = state.spawn(kind);
            state.select_only(id);
            let name = state
                .doc
                .get(id)
                .map(|object| object.name.clone())
                .unwrap_or_default();
            log.push(format!("spawned {name} (id {id})"));
        }
        Command::Delete => {
            if state.selection.is_empty() {
                log.push("delete: nothing is selected");
                return;
            }
            undo.checkpoint(state, "Console Delete");
            let removed = state.despawn_selected();
            log.push(format!("deleted {removed} object(s)"));
        }
        Command::Save => match persist::save_to(&state.doc, persist::SCENE_PATH) {
            Ok(path) => log.push(format!("saved scene to {}", path.display())),
            Err(err) => log.push(format!("could not save: {err}")),
        },
        Command::ReloadScripts => {
            let mut count = 0;
            for object in &state.doc.objects {
                if let Some(script) = &object.script {
                    reload.request(script.path.clone());
                    count += 1;
                }
            }
            state.touch();
            log.push(format!("reload requested for {count} script(s)"));
        }
        Command::Scene(path) => persist::open_path_into(state, &path, index, log),
        Command::Select(id) => {
            if state.doc.get(id).is_some() {
                state.select_only(id);
                log.push(format!("selected id {id}"));
            } else {
                log.push(format!("select: no object with id {id}"));
            }
        }
        Command::PackageInstall {
            registry,
            name,
            requirement,
        } => match install_package(
            &crate::build::workspace_dir(),
            &registry,
            &name,
            &requirement,
        ) {
            Ok(line) => log.push(line),
            Err(err) => log.push(format!("pkg: {err}")),
        },
    }
}

/// Installs a package from a registry into `root` and updates its package list.
fn install_package(
    root: &std::path::Path,
    registry: &str,
    name: &str,
    requirement: &str,
) -> Result<String, String> {
    let file = root.join(eme_editor_project::PACKAGES_FILE);
    let mut set = eme_editor_project::PackageSet::load(&file)?;
    let registry_path = std::path::PathBuf::from(registry);
    let registry_path = if registry_path.is_absolute() {
        registry_path
    } else {
        root.join(registry_path)
    };
    let manifest = set.install_from_registry(root, &registry_path, name, requirement)?;
    set.save(&file)?;
    Ok(format!(
        "installed {} {} into packages/{} from {}",
        manifest.name,
        manifest.version,
        manifest.name,
        registry_path.display()
    ))
}

/// Focuses the command field when its row is clicked.
pub fn focus_command(
    press: On<PointerPress>,
    inputs: Query<(), With<ConsoleCommandInput>>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary || !inputs.contains(press.entity) {
        return;
    }
    focus.set(press.entity, FocusCause::Pressed);
}

/// Runs the typed command on Enter and recalls history on Up/Down.
pub fn command_key(
    key: On<FocusedInput<KeyboardInput>>,
    mut fields: Query<&mut EditableText, With<ConsoleCommandInput>>,
    mut history: ResMut<CommandHistory>,
    mut state: ResMut<SceneState>,
    mut undo: ResMut<Undo>,
    mut log: ResMut<ConsoleLog>,
    mut reload: ResMut<eme_script::ScriptReload>,
    index: Res<ModelIndex>,
) {
    let input = &key.input;
    if !input.state.is_pressed() {
        return;
    }
    let entity = key.focused_entity;
    match input.key_code {
        KeyCode::ArrowUp => {
            if let Some(line) = history.previous()
                && let Ok(mut field) = fields.get_mut(entity)
            {
                field.editor.set_text(&line);
            }
        }
        KeyCode::ArrowDown => {
            if let Some(line) = history.next()
                && let Ok(mut field) = fields.get_mut(entity)
            {
                field.editor.set_text(&line);
            }
        }
        KeyCode::Tab => {
            if let Ok(mut field) = fields.get_mut(entity) {
                let line = field.value().to_string();
                let completion = complete(&line);
                if let Some(text) = completion.text {
                    field.editor.set_text(&text);
                    field.queue_edit(TextEdit::TextEnd(false));
                } else if !completion.options.is_empty() {
                    log.push(format!("completions: {}", completion.options.join(" ")));
                }
            }
        }
        KeyCode::Enter => {
            let Ok(field) = fields.get_mut(entity) else {
                return;
            };
            let line = field.value().to_string();
            let line = line.trim().to_owned();
            if line.is_empty() {
                return;
            }
            history.push(&line);
            match parse(&line) {
                Ok(command) => execute(command, &mut state, &mut undo, &mut log, &mut reload, &index),
                Err(err) => log.push(format!("console: {err}")),
            }
            if let Ok(mut field) = fields.get_mut(entity) {
                field.editor.set_text("");
            }
        }
        _ => {}
    }
}

/// Registers the command history resource.
pub fn install(app: &mut App) {
    app.init_resource::<CommandHistory>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_cube() -> (SceneState, u64) {
        let mut state = SceneState::default();
        let id = state.spawn(ObjectKind::Cube);
        state.select_only(id);
        (state, id)
    }

    #[test]
    fn commands_parse_with_kinds_and_arguments() {
        assert_eq!(parse("help").unwrap(), Command::Help);
        assert_eq!(parse("  CLEAR  ").unwrap(), Command::Clear);
        assert_eq!(parse("spawn Cube").unwrap(), Command::Spawn(ObjectKind::Cube));
        assert_eq!(parse("spawn pointlight").unwrap(), Command::Spawn(ObjectKind::PointLight));
        assert_eq!(parse("scene scenes/level2.ember").unwrap(), Command::Scene("scenes/level2.ember".into()));
        assert_eq!(parse("select 12").unwrap(), Command::Select(12));
        assert_eq!(
            parse("pkg install ../registry art@^1.2.0").unwrap(),
            Command::PackageInstall {
                registry: "../registry".into(),
                name: "art".into(),
                requirement: "^1.2.0".into(),
            }
        );
        assert_eq!(
            parse("pkg install ../registry art").unwrap(),
            Command::PackageInstall {
                registry: "../registry".into(),
                name: "art".into(),
                requirement: "*".into(),
            }
        );
        assert!(parse("pkg").is_err());
        assert!(parse("pkg install ../registry").is_err());
        assert!(parse("pkg install ../registry art@").is_err());
        assert!(parse("pkg remove art").is_err());
        assert!(parse("spawn").is_err());
        assert!(parse("spawn dragon").is_err());
        assert!(parse("scene").is_err());
        assert!(parse("select abc").is_err());
        assert!(parse("teleport").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn a_registry_install_updates_the_project() {
        let root = std::env::temp_dir().join(format!("fabr-console-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let registry = root.join("registry/packages/art-1.0.0");
        std::fs::create_dir_all(registry.join("assets")).unwrap();
        std::fs::write(
            registry.join("fabr.package.ron"),
            r#"(name: "art", version: "1.0.0", assets: "assets")"#,
        )
        .unwrap();
        std::fs::write(registry.join("assets/art.txt"), b"art").unwrap();
        std::fs::write(
            root.join("registry/fabr.registry.ron"),
            r#"(packages: [(name: "art", versions: [(version: "1.0.0", path: "packages/art-1.0.0")])])"#,
        )
        .unwrap();

        let line = install_package(&root, "registry", "art", "^1.0.0").unwrap();
        assert!(line.contains("installed art 1.0.0"), "{line}");
        assert_eq!(
            std::fs::read(root.join("packages/art/assets/art.txt")).unwrap(),
            b"art"
        );
        let set = eme_editor_project::PackageSet::load(
            &root.join(eme_editor_project::PACKAGES_FILE),
        )
        .unwrap();
        assert_eq!(set.packages.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn spawn_and_delete_change_the_document() {
        let mut state = SceneState::default();
        let mut undo = Undo::default();
        let mut log = ConsoleLog::default();
        let mut reload = eme_script::ScriptReload::default();
        let index = ModelIndex::default();
        let before = state.doc.objects.len();
        execute(Command::Spawn(ObjectKind::Cube), &mut state, &mut undo, &mut log, &mut reload, &index);
        assert_eq!(state.doc.objects.len(), before + 1);
        let id = state.primary().expect("spawn selects the new object");
        execute(Command::Delete, &mut state, &mut undo, &mut log, &mut reload, &index);
        assert!(state.doc.get(id).is_none());
        assert!(log.lines().iter().any(|line| line.contains("deleted 1 object")));
    }

    #[test]
    fn help_and_clear_write_to_the_console() {
        let mut state = SceneState::default();
        let mut undo = Undo::default();
        let mut log = ConsoleLog::default();
        let mut reload = eme_script::ScriptReload::default();
        let index = ModelIndex::default();
        execute(Command::Help, &mut state, &mut undo, &mut log, &mut reload, &index);
        assert!(log.lines().iter().any(|line| line.contains("spawn <kind>")));
        execute(Command::Clear, &mut state, &mut undo, &mut log, &mut reload, &index);
        assert!(log.lines().is_empty());
    }

    #[test]
    fn tab_completion_fills_commands_and_kinds() {
        assert_eq!(
            complete("spa"),
            Completion {
                text: Some("spawn ".into()),
                options: Vec::new()
            }
        );
        assert_eq!(complete("spawn cu").text.as_deref(), Some("spawn cube"));
        assert_eq!(complete("spawn ").options, KINDS.to_vec());
        assert_eq!(
            complete("s"),
            Completion {
                text: None,
                options: vec!["save", "spawn", "scene", "select"]
            }
        );
        assert!(complete("spawn x").options.is_empty());
        assert!(complete("nope").text.is_none());
    }

    #[test]
    fn history_recalls_previous_entries() {
        let mut history = CommandHistory::default();
        history.push("spawn cube");
        history.push("delete");
        assert_eq!(history.previous().as_deref(), Some("delete"));
        assert_eq!(history.previous().as_deref(), Some("spawn cube"));
        assert_eq!(history.next().as_deref(), Some("delete"));
        history.push("delete");
        assert_eq!(history.entries.len(), 2, "repeat of the last entry is dropped");
    }

    #[test]
    fn selecting_an_unknown_id_reports_it() {
        let (mut state, id) = state_with_cube();
        let mut undo = Undo::default();
        let mut log = ConsoleLog::default();
        let mut reload = eme_script::ScriptReload::default();
        let index = ModelIndex::default();
        state.clear_selection();
        execute(Command::Select(id), &mut state, &mut undo, &mut log, &mut reload, &index);
        assert_eq!(state.primary(), Some(id));
        execute(Command::Select(id + 99), &mut state, &mut undo, &mut log, &mut reload, &index);
        assert!(log.lines().iter().any(|line| line.contains("no object with id")));
    }
}
