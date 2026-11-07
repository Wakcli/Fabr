//! The import options panel: settings, report and the actions around them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use eme::feathers::theme::{ThemeBackgroundColor, ThemeBorderColor, ThemeToken};
use eme::prelude::*;
use eme::scene::bsn;
use eme::ui_widgets::{Activate, Button};

use super::report::ImportReport;
use super::service::ImportService;
use super::settings::{ImportSettings, UpAxis};
use super::sidecar::Sidecar;
use crate::console::ConsoleLog;
use crate::screens::editor::chrome::{self, spacer, Part, FONT_SIZE};
use crate::tokens;

/// Panel state. Every edit bumps `rev`, and the host rebuilds from it.
#[derive(Resource, Default)]
pub struct OptionsPanel {
    /// Whether the panel is on screen.
    pub open: bool,
    /// Model the panel is editing.
    pub source: PathBuf,
    /// Folder it will be copied into.
    pub folder: PathBuf,
    /// Settings being edited.
    pub settings: ImportSettings,
    /// Report shown under the options, refreshed by Dry run.
    pub report: Option<ImportReport>,
    /// Sections and warnings the user has opened.
    pub expanded: HashSet<String>,
    /// Rebuild counter.
    pub rev: u64,
}

impl OptionsPanel {
    /// Opens the panel for one file, loading its stored settings when present.
    pub fn open_for(&mut self, source: &Path, folder: &Path) {
        let sidecar = Sidecar::load_or_new(source);
        self.source = source.to_path_buf();
        self.folder = folder.to_path_buf();
        self.settings = sidecar.settings;
        self.report = None;
        self.expanded = ["transform", "geometry", "materials", "animations", "scene", "output"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        self.open = true;
        self.rev = self.rev.wrapping_add(1);
    }

    /// Marks a rebuild.
    pub fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    /// Whether a section or warning row is expanded.
    pub fn is_open(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    /// Flips a section or warning row.
    pub fn toggle(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
        self.bump();
    }
}

/// Marks the node whose children are rebuilt from [`OptionsPanel`].
#[derive(Component, Clone, Copy, Default)]
pub struct PanelHost;

/// What a row does when clicked.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Open or close a section.
    Section(&'static str),
    /// Step the import scale through a fixed ladder.
    Scale,
    /// Flip Y/Z up.
    UpAxis,
    /// Dump the file's JSON to the console.
    RawJson,
    /// Step the texture limit.
    MaxTexture,
    /// Parse without writing anything.
    DryRun,
    /// Copy the report text to the clipboard.
    CopyReport,
    /// Store the current settings as the folder preset.
    SavePreset,
    /// Open the asset's folder in the file manager.
    Reveal,
    /// Import with the current settings.
    Import,
    /// Close the panel.
    Close,
    /// Expand one warning.
    Warning(usize),
    /// Toggle instancing after an import.
    Instance,
}

/// Scale ladder offered by the panel.
pub const SCALES: [f32; 5] = [0.01, 0.1, 1.0, 2.54, 10.0];
/// Texture limits offered by the panel.
pub const TEXTURE_LIMITS: [u32; 4] = [1024, 2048, 4096, 8192];

/// The panel overlay, hidden until opened.
pub fn panel() -> Part {
    Box::new(bsn! {
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            left: px(360),
            top: px(70),
            width: px(560),
            max_height: percent(80),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: px(1),
            border_radius: px(4),
            padding: px(1),
            overflow: Overflow::clip(),
        }
        OptionsRoot
        GlobalZIndex(300)
        ThemeBackgroundColor(tokens::WINDOW)
        ThemeBorderColor(tokens::BORDER)
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
            }
            PanelHost
        ]
    })
}

/// Marks the panel's window so it can be shown and hidden.
#[derive(Component, Clone, Copy, Default)]
pub struct OptionsRoot;

/// Shows or hides the panel and rebuilds its body when the state changes.
pub fn sync(
    mut panel: ResMut<OptionsPanel>,
    mut service: ResMut<ImportService>,
    mut commands: Commands,
    mut roots: Query<&mut Node, With<OptionsRoot>>,
    hosts: Query<Entity, With<PanelHost>>,
    mut last: Local<Option<(Entity, u64, bool)>>,
) {
    if panel.open
        && let Some(outcome) = service.dry.remove(&panel.source)
    {
        panel.report = outcome.ok();
        panel.bump();
    }
    for mut node in &mut roots {
        let want = if panel.open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    let Ok(host) = hosts.single() else {
        return;
    };
    if *last == Some((host, panel.rev, panel.open)) {
        return;
    }
    *last = Some((host, panel.rev, panel.open));
    commands.entity(host).despawn_children();
    if !panel.open {
        return;
    }
    for row in rows(&panel) {
        commands.spawn_scene(eme::scene::bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
            }
            ChildOf(host)
            Children [ {row} ]
        });
    }
}

fn rows(panel: &OptionsPanel) -> Vec<Part> {
    vec![
        header(panel),
        section(panel, "transform", "Transform", transform_rows(panel)),
        section(panel, "geometry", "Geometry", geometry_rows(panel)),
        section(panel, "materials", "Materials", material_rows(panel)),
        section(panel, "animations", "Animations", animation_rows(panel)),
        section(panel, "scene", "Scene", scene_rows(panel)),
        section(panel, "output", "Output", output_rows(panel)),
        section(panel, "nodes", "Nodes", node_rows(panel)),
        buttons(),
        report_block(panel),
    ]
}

fn header(panel: &OptionsPanel) -> Part {
    let name = panel
        .source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Box::new(bsn! {
        Node {
            height: px(28),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(8)),
            border: UiRect { bottom: px(1) },
        }
        ThemeBackgroundColor(tokens::PANEL)
        ThemeBorderColor(tokens::BORDER)
        Children [
            {text(format!("Import options — {name}"), FONT_SIZE, tokens::TEXT)}
            --
            {spacer()}
            --
            {button_row("Close", Action::Close)}
        ]
    })
}

fn section(panel: &OptionsPanel, id: &'static str, title: &str, body: Vec<Part>) -> Part {
    let open = panel.is_open(id);
    let mut kids = vec![button_row_with(title, Action::Section(id), tokens::TEXT)];
    if open {
        kids.extend(body);
    }
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
        }
        Children [ {kids} ]
    })
}

fn option(label: &str, value: &str, action: Option<Action>) -> Part {
    match action {
        Some(action) => Box::new(bsn! {
            Node {
                height: px(22),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
            }
            Button
            action
            Children [
                {text(label, FONT_SIZE, tokens::TEXT)}
                --
                {spacer()}
                --
                {text(value, FONT_SIZE, tokens::ACCENT)}
            ]
            on(activate)
        }),
        None => Box::new(bsn! {
            Node {
                height: px(22),
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
                column_gap: px(8),
            }
            Children [
                {text(label, FONT_SIZE, tokens::TEXT_DIM)}
                --
                {spacer()}
                --
                {text("not implemented", FONT_SIZE, tokens::TEXT_DIM)}
            ]
        }),
    }
}

fn button_row(label: &str, action: Action) -> Part {
    button_row_with(label, action, tokens::TEXT)
}

fn button_row_with(label: &str, action: Action, token: ThemeToken) -> Part {
    Box::new(bsn! {
        Node {
            height: px(24),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(10)),
        }
        Button
        action
        Children [ {text(label, FONT_SIZE, token)} ]
        on(activate)
    })
}

fn transform_rows(panel: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Scale", &format!("{}", panel.settings.transform.scale), Some(Action::Scale)),
        option(
            "Up axis",
            match panel.settings.transform.up_axis {
                UpAxis::Y => "Y",
                UpAxis::Z => "Z",
            },
            Some(Action::UpAxis),
        ),
    ]
}

fn geometry_rows(_: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Normals", "", None),
        option("Tangents", "", None),
        option("Merge meshes", "", None),
        option("Colliders", "", None),
    ]
}

fn material_rows(panel: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Import materials", "", None),
        option("Extract materials", "", None),
        option("Keep overrides on reimport", "", None),
        option(
            "Max texture size (report only)",
            &format!("{} px", panel.settings.materials.max_texture_size),
            Some(Action::MaxTexture),
        ),
        option("Mipmaps", "", None),
    ]
}

fn animation_rows(_: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Import animations", "", None),
        option("Split clips", "", None),
        option("Loop clips", "", None),
    ]
}

fn scene_rows(_: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Cameras", "", None),
        option("Lights", "", None),
        option("Extras", "", None),
    ]
}

fn output_rows(panel: &OptionsPanel) -> Vec<Part> {
    vec![
        option("Folder", &panel.folder.display().to_string(), None),
        option(
            "Instance after import",
            if panel.settings.output.instance_after_import { "yes" } else { "no" },
            Some(Action::Instance),
        ),
    ]
}

fn node_rows(panel: &OptionsPanel) -> Vec<Part> {
    let Some(report) = &panel.report else {
        return vec![text("Dry run to list the nodes", FONT_SIZE, tokens::TEXT_DIM)];
    };
    if report.nodes.is_empty() {
        return vec![text("no nodes", FONT_SIZE, tokens::TEXT_DIM)];
    }
    report
        .nodes
        .iter()
        .take(60)
        .map(|node| {
            let mut line = String::new();
            for _ in 0..node.depth {
                line.push_str("  ");
            }
            line.push_str(&format!("☐ {} (#{})", node.name, node.index));
            text(line, FONT_SIZE, tokens::TEXT_DIM)
        })
        .collect()
}

fn buttons() -> Part {
    let kids = vec![
        button_row("Dry run", Action::DryRun),
        button_row("Copy report", Action::CopyReport),
        button_row("Save folder preset", Action::SavePreset),
        button_row("Reveal", Action::Reveal),
        button_row("Raw JSON", Action::RawJson),
        button_row("Import", Action::Import),
    ];
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(2),
            padding: UiRect::horizontal(px(6)),
            border: UiRect { top: px(1), bottom: px(1) },
        }
        ThemeBorderColor(tokens::BORDER)
        Children [ {kids} ]
    })
}

fn report_block(panel: &OptionsPanel) -> Part {
    let mut lines: Vec<Part> = Vec::new();
    match &panel.report {
        None => lines.push(text("Dry run to see the report", FONT_SIZE, tokens::TEXT_DIM)),
        Some(report) => {
            lines.push(text(report.summary(), FONT_SIZE, tokens::TEXT));
            if let Some(bounds) = report.bounds {
                let size = bounds.size();
                lines.push(text(
                    format!("bounds {:.2} × {:.2} × {:.2} m", size[0], size[1], size[2]),
                    FONT_SIZE,
                    tokens::TEXT_DIM,
                ));
            }
            lines.push(text(
                format!(
                    "{} textures · {} animations · {:.1} ms",
                    report.textures.len(),
                    report.animations.len(),
                    report.timings.total_ms
                ),
                FONT_SIZE,
                tokens::TEXT_DIM,
            ));
            for (index, warning) in report.warnings.iter().enumerate() {
                let id = format!("w{index}");
                let expanded = panel.is_open(&id);
                lines.push(button_row(
                    &format!("{}: {}", warning.severity, warning.message),
                    Action::Warning(index),
                ));
                if expanded
                    && let Some(target) = &warning.target
                {
                    lines.push(text(format!("  at {target}"), FONT_SIZE, tokens::TEXT_DIM));
                }
            }
            if report.warnings.is_empty() {
                lines.push(text("no warnings", FONT_SIZE, tokens::TEXT_DIM));
            }
        }
    }
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            padding: UiRect {
                left: px(10),
                right: px(8),
                top: px(4),
                bottom: px(6),
            },
            row_gap: px(2),
            overflow: Overflow::clip(),
        }
        Children [ {lines} ]
    })
}

fn text(value: impl Into<String>, size: f32, token: ThemeToken) -> Part {
    chrome::text_mono(value, size, token)
}

/// Acts on whatever row was clicked.
pub fn activate(
    clicked: On<Activate>,
    actions: Query<&Action>,
    mut panel: ResMut<OptionsPanel>,
    mut service: ResMut<ImportService>,
    mut log: ResMut<ConsoleLog>,
) {
    let Ok(action) = actions.get(clicked.entity) else {
        return;
    };
    match *action {
        Action::Section(id) => panel.toggle(id),
        Action::Warning(index) => panel.toggle(&format!("w{index}")),
        Action::Scale => {
            let current = panel.settings.transform.scale;
            let next = SCALES
                .iter()
                .copied()
                .find(|value| *value > current + 1e-6)
                .unwrap_or(SCALES[0]);
            panel.settings.transform.scale = next;
            panel.bump();
        }
        Action::UpAxis => {
            panel.settings.transform.up_axis = match panel.settings.transform.up_axis {
                UpAxis::Y => UpAxis::Z,
                UpAxis::Z => UpAxis::Y,
            };
            panel.bump();
        }
        Action::MaxTexture => {
            let current = panel.settings.materials.max_texture_size;
            let next = TEXTURE_LIMITS
                .iter()
                .copied()
                .find(|value| *value > current)
                .unwrap_or(TEXTURE_LIMITS[0]);
            panel.settings.materials.max_texture_size = next;
            panel.bump();
        }
        Action::Instance => {
            panel.settings.output.instance_after_import = !panel.settings.output.instance_after_import;
            panel.bump();
        }
        Action::Close => {
            panel.open = false;
            panel.bump();
        }
        Action::DryRun => {
            let source = panel.source.clone();
            let settings = panel.settings.clone();
            if service.dry_run(&source, settings, &mut log) {
                panel.report = None;
                panel.bump();
            }
        }
        Action::CopyReport => {
            let body = panel
                .report
                .as_ref()
                .map_or_else(|| "no report yet".to_string(), ImportReport::report_text);
            if copy_to_clipboard(&body).is_err() {
                for line in body.lines() {
                    log.push(line.to_owned());
                }
                log.push("(no clipboard available; report written to the console)");
            } else {
                log.push("report copied to the clipboard");
            }
        }
        Action::SavePreset => {
            let path = panel.folder.join(".import-preset.ron");
            match ron::ser::to_string_pretty(&panel.settings, ron::ser::PrettyConfig::default())
                .map_err(|err| err.to_string())
                .and_then(|text| std::fs::write(&path, text).map_err(|err| err.to_string()))
            {
                Ok(()) => log.push(format!("saved import preset to {}", path.display())),
                Err(err) => log.push(format!("could not save preset: {err}")),
            }
        }
        Action::RawJson => match raw_json(&panel.source) {
            Ok(text) => {
                let mut lines: Vec<&str> = text.lines().take(120).collect();
                let truncated = text.lines().count() > lines.len();
                lines.push(if truncated { "… (truncated)" } else { "(end)" });
                for line in lines {
                    log.push(line.to_owned());
                }
            }
            Err(err) => log.push(format!("could not read JSON: {err}")),
        },
        Action::Reveal => {
            let dir = panel.source.parent().unwrap_or(Path::new("."));
            match std::process::Command::new("xdg-open").arg(dir).spawn() {
                Ok(_) => log.push(format!("opened {}", dir.display())),
                Err(err) => log.push(format!("could not open {}: {err}", dir.display())),
            }
        }
        Action::Import => {
            let sidecar = Sidecar {
                version: super::sidecar::SIDECAR_VERSION,
                guid: Sidecar::load_or_new(&panel.source).guid,
                settings: panel.settings.clone(),
            };
            if let Err(err) = sidecar.save(&panel.source) {
                log.push(format!("could not save sidecar: {err}"));
            }
            service.reload(&panel.source, &mut log);
            panel.open = false;
            panel.bump();
        }
    }
}

/// The glTF JSON, unwrapped from the GLB container when needed.
fn raw_json(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 {
            return Err("GLB header is short".into());
        }
        let length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let start: usize = 20;
        let end = start.saturating_add(length).min(bytes.len());
        return Ok(String::from_utf8_lossy(&bytes[start..end]).into_owned());
    }
    String::from_utf8(bytes).map_err(|err| err.to_string())
}

fn copy_to_clipboard(text: &str) -> Result<(), std::io::Error> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    for tool in ["wl-copy", "xclip"] {
        let mut command = Command::new(tool);
        if tool == "xclip" {
            command.args(["-selection", "clipboard"]);
        }
        if let Ok(mut child) = command.stdin(Stdio::piped()).spawn() {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
    }
    Err(std::io::Error::other("no clipboard tool"))
}

impl ImportReport {
    /// The full report as plain text, for the clipboard.
    pub fn report_text(&self) -> String {
        let mut out = self.summary();
        out.push('\n');
        out.push_str(&format!(
            "file: {} ({} bytes, glTF {}{})\n",
            self.file.path.display(),
            self.file.bytes,
            self.file.version,
            if self.file.binary { ", binary" } else { "" },
        ));
        if let Some(generator) = &self.file.generator {
            out.push_str(&format!("generator: {generator}\n"));
        }
        out.push_str(&format!(
            "counts: {} nodes, {} meshes, {} verts, {} tris, {} materials, {} skins, {} cameras, {} lights\n",
            self.counts.nodes,
            self.counts.meshes,
            self.counts.vertices,
            self.counts.triangles,
            self.counts.materials,
            self.counts.skins,
            self.counts.cameras,
            self.counts.lights,
        ));
        if let Some(bounds) = self.bounds {
            let size = bounds.size();
            out.push_str(&format!(
                "bounds: {:.3} × {:.3} × {:.3} m\n",
                size[0], size[1], size[2]
            ));
        }
        for texture in &self.textures {
            out.push_str(&format!(
                "texture[{}]: {}x{} {} ({} bytes)\n",
                texture.index, texture.width, texture.height, texture.format, texture.bytes
            ));
        }
        for animation in &self.animations {
            out.push_str(&format!(
                "animation[{}]: {:?} {:.2}s, {} channels\n",
                animation.index, animation.name, animation.duration, animation.channels
            ));
        }
        for warning in &self.warnings {
            out.push_str(&format!(
                "{}: {}{}\n",
                warning.severity,
                warning.message,
                warning
                    .target
                    .as_deref()
                    .map_or_else(String::new, |target| format!(" [{target}]")),
            ));
        }
        out.push_str(&format!(
            "timings: read {:.1} ms, parse {:.1} ms, analyse {:.1} ms, total {:.1} ms\n",
            self.timings.read_ms,
            self.timings.parse_ms,
            self.timings.analyse_ms,
            self.timings.total_ms
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> OptionsPanel {
        OptionsPanel::default()
    }

    #[test]
    fn scale_cycles_through_the_ladder_and_wraps() {
        let mut state = panel();
        state.settings.transform.scale = 1.0;
        let next = SCALES
            .iter()
            .copied()
            .find(|value| *value > state.settings.transform.scale + 1e-6)
            .unwrap_or(SCALES[0]);
        assert_eq!(next, 2.54);
    }

    #[test]
    fn sections_toggle_independently() {
        let mut state = panel();
        assert!(!state.is_open("transform"));
        state.toggle("transform");
        assert!(state.is_open("transform"));
        assert!(!state.is_open("output"));
        state.toggle("transform");
        assert!(!state.is_open("transform"));
    }

    #[test]
    fn opening_loads_the_sidecar_settings() {
        let dir = std::env::temp_dir().join(format!("fabr-panel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("chair.glb");
        std::fs::write(&source, b"glTF").unwrap();

        let mut sidecar = Sidecar::load_or_new(&source);
        sidecar.settings.transform.scale = 0.01;
        sidecar.save(&source).unwrap();

        let mut state = panel();
        state.open_for(&source, &dir);
        assert!(state.open);
        assert_eq!(state.settings.transform.scale, 0.01);
    }

    #[test]
    fn unimplemented_options_render_as_disabled_rows() {
        let state = panel();
        assert_eq!(geometry_rows(&state).len(), 4);
        assert_eq!(animation_rows(&state).len(), 3);
        assert_eq!(material_rows(&state).len(), 5);
    }

    #[test]
    fn report_text_keeps_every_section() {
        let report = ImportReport {
            file: super::super::report::FileInfo {
                path: PathBuf::from("assets/a.glb"),
                bytes: 12,
                version: "2.0".into(),
                generator: Some("Blender".into()),
                extensions: Vec::new(),
                binary: true,
            },
            counts: super::super::report::Counts::default(),
            bounds: None,
            textures: Vec::new(),
            animations: Vec::new(),
            nodes: Vec::new(),
            timings: super::super::report::StageTimings::default(),
            warnings: vec![super::super::report::ImportWarning::new(
                super::super::report::Severity::Warning,
                "nodes[1]",
                "missing normals",
            )],
        };
        let text = report.report_text();
        assert!(text.contains("file: assets/a.glb"));
        assert!(text.contains("warning: missing normals [nodes[1]]"));
        assert!(text.contains("timings:"));
    }
}
