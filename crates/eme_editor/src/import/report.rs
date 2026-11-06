//! What one import produced, and what went wrong while producing it.

use core::fmt;
use std::path::PathBuf;

/// How much an import warning matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing, nothing to fix.
    Info,
    /// The import succeeded but the asset will look or behave differently.
    Warning,
    /// Something was dropped or could not be read.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// One problem found during import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportWarning {
    /// How bad it is.
    pub severity: Severity,
    /// What was found, in plain language.
    pub message: String,
    /// Where it was found: a node path, mesh name, texture name or file offset.
    pub target: Option<String>,
}

impl ImportWarning {
    /// A warning attached to a place in the asset.
    pub fn new(severity: Severity, target: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            target: Some(target.into()),
        }
    }

    /// A warning about the file as a whole.
    pub fn file(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            target: None,
        }
    }
}

/// Facts about the source file itself.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    /// Path that was read.
    pub path: PathBuf,
    /// Size on disk.
    pub bytes: u64,
    /// `asset.version` from the document.
    pub version: String,
    /// `asset.generator`, when present.
    pub generator: Option<String>,
    /// `extensionsUsed` from the document.
    pub extensions: Vec<String>,
    /// True when the file was a binary GLB.
    pub binary: bool,
}

/// How much of each thing the file holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Scene graph nodes.
    pub nodes: usize,
    /// Mesh objects.
    pub meshes: usize,
    /// Position vertices across every primitive.
    pub vertices: usize,
    /// Triangles across every primitive.
    pub triangles: usize,
    /// Material definitions.
    pub materials: usize,
    /// Skins.
    pub skins: usize,
    /// Cameras.
    pub cameras: usize,
    /// Punctual lights.
    pub lights: usize,
}

/// Axis-aligned bounds in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    /// Minimum corner.
    pub min: [f32; 3],
    /// Maximum corner.
    pub max: [f32; 3],
}

impl Bounds {
    /// Extent along each axis.
    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    /// Longest edge, the number people quote for "how big is it".
    pub fn longest(&self) -> f32 {
        self.size().into_iter().fold(0.0, f32::max)
    }
}

/// One node of the scene graph, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInfo {
    /// glTF node index; stable inside the file and the key Phase 5 will use.
    pub index: usize,
    /// Node name, or `GltfNode{index}` when the file has none.
    pub name: String,
    /// Nesting depth, roots at 0.
    pub depth: usize,
}

/// One texture that came with the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureInfo {
    /// Image index in the document.
    pub index: usize,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixel format as reported by the decoder.
    pub format: String,
    /// Decoded size in bytes.
    pub bytes: usize,
}

/// One animation clip.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationInfo {
    /// Animation index in the document.
    pub index: usize,
    /// Name from the document, when it has one.
    pub name: Option<String>,
    /// Length in seconds, from the longest sampler input.
    pub duration: f32,
    /// Sampler channel count.
    pub channels: usize,
}

/// Wall time spent in each stage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageTimings {
    /// Reading the file from disk.
    pub read_ms: f32,
    /// Parsing and loading buffers and images.
    pub parse_ms: f32,
    /// Walking the document for counts, bounds and warnings.
    pub analyse_ms: f32,
    /// Everything.
    pub total_ms: f32,
}

/// Everything one import learned.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportReport {
    /// Source file facts.
    pub file: FileInfo,
    /// Object counts.
    pub counts: Counts,
    /// Bounds in metres, when the file has geometry.
    pub bounds: Option<Bounds>,
    /// Decoded textures.
    pub textures: Vec<TextureInfo>,
    /// Animation clips.
    pub animations: Vec<AnimationInfo>,
    /// Scene graph nodes, depth first.
    pub nodes: Vec<NodeInfo>,
    /// Per-stage timings.
    pub timings: StageTimings,
    /// Problems found, worst last.
    pub warnings: Vec<ImportWarning>,
}

impl ImportReport {
    /// Warnings at or above `severity`.
    pub fn warnings_at(&self, severity: Severity) -> impl Iterator<Item = &ImportWarning> {
        self.warnings.iter().filter(move |w| w.severity >= severity)
    }

    /// The one line the console shows after an import.
    pub fn summary(&self) -> String {
        let name = self
            .file
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.file.path.display().to_string());
        let size = match self.bounds {
            Some(bounds) => format!("{:.2} m", bounds.longest()),
            None => "no geometry".into(),
        };
        let counts = &self.counts;
        let warns = self.warnings_at(Severity::Warning).count();
        format!(
            "imported {name} — {} nodes, {} meshes, {} verts, {} tris, {} materials, {} textures, {} anims, {size}, {:.0} ms, {warns} warnings",
            counts.nodes,
            counts.meshes,
            counts.vertices,
            counts.triangles,
            counts.materials,
            self.textures.len(),
            self.animations.len(),
            self.timings.total_ms,
        )
    }
}

/// Why an import could not produce a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The file could not be read.
    Io(String),
    /// The glTF parser rejected the file.
    Parse(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Parse(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> ImportReport {
        ImportReport {
            file: FileInfo {
                path: PathBuf::from("assets/models/thing.glb"),
                bytes: 4096,
                version: "2.0".into(),
                generator: Some("Blender".into()),
                extensions: Vec::new(),
                binary: true,
            },
            counts: Counts {
                nodes: 3,
                meshes: 2,
                vertices: 120,
                triangles: 40,
                materials: 1,
                ..Counts::default()
            },
            bounds: Some(Bounds {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 2.0, 0.5],
            }),
            textures: Vec::new(),
            animations: Vec::new(),
            nodes: Vec::new(),
            timings: StageTimings {
                total_ms: 12.0,
                ..StageTimings::default()
            },
            warnings: vec![
                ImportWarning::file(Severity::Info, "tangents generated"),
                ImportWarning::new(Severity::Warning, "nodes[2]", "non-uniform scale"),
            ],
        }
    }

    #[test]
    fn bounds_report_the_longest_edge() {
        let bounds = report().bounds.unwrap();
        assert_eq!(bounds.longest(), 2.0);
    }

    #[test]
    fn summary_is_one_line_with_the_important_numbers() {
        let line = report().summary();
        assert!(!line.contains('\n'), "summary must be a single console line");
        assert!(line.starts_with("imported thing.glb"));
        assert!(line.contains("3 nodes"));
        assert!(line.contains("40 tris"));
        assert!(line.contains("2.00 m"));
        assert!(line.contains("1 warnings"));
    }

    #[test]
    fn warnings_filter_by_severity() {
        let report = report();
        assert_eq!(report.warnings_at(Severity::Warning).count(), 1);
        assert_eq!(report.warnings_at(Severity::Info).count(), 2);
    }
}
