//! Reads a glTF or GLB file and reports what is inside it.
//!
//! This is the only place that parses model files. Every entry point calls
//! [`import`], so the report, the warnings and the failure modes cannot drift
//! between the drop handler, the menu and the inspector.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use gltf::Semantic;

use super::report::{
    AnimationInfo, Bounds, Counts, FileInfo, ImportError, ImportReport, ImportWarning, Severity,
    StageTimings, TextureInfo,
};
use super::settings::{ImportSettings, UpAxis};

/// Extensions this importer understands; anything else becomes a warning.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "KHR_lights_punctual",
    "KHR_materials_emissive_strength",
    "KHR_materials_ior",
    "KHR_materials_specular",
    "KHR_materials_transmission",
    "KHR_materials_unlit",
    "KHR_materials_volume",
    "KHR_texture_transform",
];

/// Below this triangle area the geometry is treated as degenerate.
const DEGENERATE_AREA: f32 = 1e-9;

/// Reads `path` and returns everything worth knowing about it.
///
/// Fails only when the file cannot be read or parsed; a file that parses but
/// has problems returns a report whose warnings explain them.
pub fn import(path: &Path, settings: &ImportSettings) -> Result<ImportReport, ImportError> {
    let started = Instant::now();

    let read_start = Instant::now();
    let bytes = std::fs::read(path).map_err(|err| ImportError::Io(format!("{}: {err}", path.display())))?;
    let read_ms = read_start.elapsed().as_secs_f32() * 1000.0;

    let parse_start = Instant::now();
    let (doc, buffers, images) =
        gltf::import(path).map_err(|err| ImportError::Parse(err.to_string()))?;
    let parse_ms = parse_start.elapsed().as_secs_f32() * 1000.0;

    let analyse_start = Instant::now();
    let json = doc.as_json();
    let mut warnings = Vec::new();

    let binary = bytes.starts_with(b"glTF");
    if !binary && path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("gltf")) {
        warnings.push(ImportWarning::file(
            Severity::Info,
            "file has no .gltf or .glb extension",
        ));
    }

    let extensions = json.extensions_used.clone();
    for name in &extensions {
        if !SUPPORTED_EXTENSIONS.contains(&name.as_str()) {
            warnings.push(ImportWarning::new(
                Severity::Warning,
                name.clone(),
                "extension is not supported and will be ignored",
            ));
        }
    }

    let mut counts = Counts {
        nodes: json.nodes.len(),
        meshes: json.meshes.len(),
        materials: json.materials.len(),
        skins: json.skins.len(),
        cameras: json.cameras.len(),
        lights: json
            .extensions
            .as_ref()
            .and_then(|ext| ext.khr_lights_punctual.as_ref())
            .map_or(0, |lights| lights.lights.len()),
        ..Counts::default()
    };

    let mut names: HashMap<String, usize> = HashMap::new();

    for node in doc.nodes() {
        if let Some(name) = node.name() {
            *names.entry(name.to_owned()).or_default() += 1;
        }
        let scale = node.transform().decomposed().2;
        if (scale[0] - scale[1]).abs() > 1e-6
            || (scale[1] - scale[2]).abs() > 1e-6
            || (scale[0] - scale[2]).abs() > 1e-6
        {
            warnings.push(ImportWarning::new(
                Severity::Warning,
                node_label(&node),
                format!(
                    "non-uniform scale ({:.3}, {:.3}, {:.3})",
                    scale[0], scale[1], scale[2]
                ),
            ));
        }
    }
    for (name, count) in &names {
        if *count > 1 {
            warnings.push(ImportWarning::new(
                Severity::Warning,
                name.clone(),
                format!("{count} nodes share this name"),
            ));
        }
    }

    for mesh in doc.meshes() {
        for primitive in mesh.primitives() {
            let label = mesh
                .name()
                .map_or_else(|| format!("mesh[{}]", mesh.index()), str::to_owned);
            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));

            if let Some(positions) = primitive.get(&Semantic::Positions) {
                counts.vertices += positions.count();
            } else {
                warnings.push(ImportWarning::new(
                    Severity::Error,
                    label.clone(),
                    "primitive has no POSITION attribute",
                ));
            }

            if primitive.get(&Semantic::Normals).is_none() {
                warnings.push(ImportWarning::new(
                    Severity::Warning,
                    label.clone(),
                    "missing normals; lighting will be flat",
                ));
            }
            if primitive.get(&Semantic::Tangents).is_none() {
                warnings.push(ImportWarning::new(
                    Severity::Info,
                    label.clone(),
                    "missing tangents; normal maps will not work",
                ));
            }

            let vertex_count = primitive
                .get(&Semantic::Positions)
                .map_or(0, |accessor| accessor.count());
            let triangles = primitive
                .indices()
                .map_or(vertex_count / 3, |accessor| accessor.count() / 3);
            counts.triangles += triangles;

            if let Some(flat) = reader.read_positions() {
                let positions: Vec<[f32; 3]> = flat.collect();
                let indices: Vec<u32> = reader
                    .read_indices()
                    .map(|read| read.into_u32().collect())
                    .unwrap_or_else(|| (0..positions.len() as u32).collect());
                let degenerate = count_degenerate(&positions, &indices);
                if degenerate > 0 {
                    warnings.push(ImportWarning::new(
                        Severity::Warning,
                        label.clone(),
                        format!("{degenerate} degenerate triangles"),
                    ));
                }
            }
        }
    }

    let mut bounds = world_bounds(&doc);
    if let Some(bounds) = bounds.as_mut() {
        for axis in 0..3 {
            bounds.min[axis] *= settings.transform.scale;
            bounds.max[axis] *= settings.transform.scale;
        }
        if settings.transform.up_axis == UpAxis::Z {
            bounds.min.swap(1, 2);
            bounds.max.swap(1, 2);
        }
    }

    let textures: Vec<TextureInfo> = images
        .iter()
        .enumerate()
        .map(|(index, image)| TextureInfo {
            index,
            width: image.width,
            height: image.height,
            format: format!("{:?}", image.format),
            bytes: image.pixels.len(),
        })
        .collect();
    let limit = settings.materials.max_texture_size;
    for texture in &textures {
        if texture.width > limit || texture.height > limit {
            warnings.push(ImportWarning::new(
                Severity::Warning,
                format!("image[{}]", texture.index),
                format!(
                    "{}x{} exceeds the {limit} px limit",
                    texture.width, texture.height
                ),
            ));
        }
        if texture.width == 0 || texture.height == 0 {
            warnings.push(ImportWarning::new(
                Severity::Error,
                format!("image[{}]", texture.index),
                "texture could not be decoded",
            ));
        }
    }

    for (index, buffer) in buffers.iter().enumerate() {
        if buffer.0.is_empty() {
            warnings.push(ImportWarning::new(
                Severity::Error,
                format!("buffer[{index}]"),
                "buffer data is empty; the file may reference missing files",
            ));
        }
    }

    let animations: Vec<AnimationInfo> = doc
        .animations()
        .enumerate()
        .map(|(index, animation)| {
            let duration = animation
                .samplers()
                .filter_map(|sampler| sampler.input().max().and_then(|max| max.as_array().and_then(|values| values.first().and_then(gltf::json::Value::as_f64))))
                .fold(0.0_f64, f64::max);
            AnimationInfo {
                index,
                name: animation.name().map(str::to_owned),
                duration: duration as f32,
                channels: animation.channels().count(),
            }
        })
        .collect();

    warnings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.target.cmp(&b.target)));
    let analyse_ms = analyse_start.elapsed().as_secs_f32() * 1000.0;

    Ok(ImportReport {
        file: FileInfo {
            path: path.to_path_buf(),
            bytes: bytes.len() as u64,
            version: json.asset.version.clone(),
            generator: json.asset.generator.clone(),
            extensions,
            binary,
        },
        counts,
        bounds,
        textures,
        animations,
        timings: StageTimings {
            read_ms,
            parse_ms,
            analyse_ms,
            total_ms: started.elapsed().as_secs_f32() * 1000.0,
        },
        warnings,
    })
}

fn node_label(node: &gltf::Node<'_>) -> String {
    node.name()
        .map_or_else(|| format!("node[{}]", node.index()), str::to_owned)
}

fn accessor_bounds(accessor: &gltf::Accessor<'_>) -> Option<([f32; 3], [f32; 3])> {
    let min = accessor.min()?.as_array()?.clone();
    let max = accessor.max()?.as_array()?.clone();
    if min.len() < 3 || max.len() < 3 {
        return None;
    }
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for axis in 0..3 {
        lo[axis] = min[axis].as_f64()? as f32;
        hi[axis] = max[axis].as_f64()? as f32;
    }
    Some((lo, hi))
}

/// Union of every mesh primitive's bounds after its node's world transform.
///
/// The hierarchy matters: a child mesh inherits its parents' translation,
/// rotation and scale, so the report has to walk up before transforming corners.
fn world_bounds(doc: &gltf::Document) -> Option<Bounds> {
    let mut parents: HashMap<usize, usize> = HashMap::new();
    for node in doc.nodes() {
        for child in node.children() {
            parents.insert(child.index(), node.index());
        }
    }

    let mut bounds: Option<Bounds> = None;
    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else {
            continue;
        };
        let world = world_matrix(doc, &node, &parents);
        for primitive in mesh.primitives() {
            let Some(positions) = primitive.get(&Semantic::Positions) else {
                continue;
            };
            let Some((lo, hi)) = accessor_bounds(&positions) else {
                continue;
            };
            for corner in corners(lo, hi) {
                let point = transform_point(&world, corner);
                bounds = Some(union_point(bounds, point));
            }
        }
    }
    bounds
}

fn world_matrix(
    doc: &gltf::Document,
    node: &gltf::Node<'_>,
    parents: &HashMap<usize, usize>,
) -> [[f32; 4]; 4] {
    let mut matrix = node.transform().matrix();
    let mut current = parents.get(&node.index()).copied();
    while let Some(index) = current {
        let Some(parent) = doc.nodes().find(|candidate| candidate.index() == index) else {
            break;
        };
        matrix = multiply(parent.transform().matrix(), matrix);
        current = parents.get(&index).copied();
    }
    matrix
}

fn multiply(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0_f32; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for step in 0..4 {
                sum += a[step][row] * b[column][step];
            }
            out[column][row] = sum;
        }
    }
    out
}

fn transform_point(matrix: &[[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = point;
    [
        matrix[0][0] * x + matrix[1][0] * y + matrix[2][0] * z + matrix[3][0],
        matrix[0][1] * x + matrix[1][1] * y + matrix[2][1] * z + matrix[3][1],
        matrix[0][2] * x + matrix[1][2] * y + matrix[2][2] * z + matrix[3][2],
    ]
}

fn corners(lo: [f32; 3], hi: [f32; 3]) -> [[f32; 3]; 8] {
    [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [lo[0], hi[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [lo[0], lo[1], hi[2]],
        [hi[0], lo[1], hi[2]],
        [lo[0], hi[1], hi[2]],
        [hi[0], hi[1], hi[2]],
    ]
}

fn union_point(current: Option<Bounds>, point: [f32; 3]) -> Bounds {
    match current {
        None => Bounds {
            min: point,
            max: point,
        },
        Some(bounds) => Bounds {
            min: [
                bounds.min[0].min(point[0]),
                bounds.min[1].min(point[1]),
                bounds.min[2].min(point[2]),
            ],
            max: [
                bounds.max[0].max(point[0]),
                bounds.max[1].max(point[1]),
                bounds.max[2].max(point[2]),
            ],
        },
    }
}

fn count_degenerate(positions: &[[f32; 3]], indices: &[u32]) -> usize {
    let mut degenerate = 0;
    for &triangle in indices.as_chunks::<3>().0 {
        let (Some(&a), Some(&b), Some(&c)) = (
            positions.get(triangle[0] as usize),
            positions.get(triangle[1] as usize),
            positions.get(triangle[2] as usize),
        ) else {
            degenerate += 1;
            continue;
        };
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let area = 0.5
            * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        if area <= DEGENERATE_AREA {
            degenerate += 1;
        }
    }
    degenerate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::settings::ImportSettings;
    use std::path::PathBuf;

    const TRI_GLTF: &str = r#"{
  "asset": {"version": "2.0", "generator": "unit-test"},
  "extensionsUsed": ["KHR_draco_mesh_compression"],
  "scene": 0,
  "scenes": [{"nodes": [0, 1]}],
  "nodes": [
    {"name": "Cube", "mesh": 0},
    {"name": "Cube", "scale": [1.0, 2.0, 1.0]}
  ],
  "meshes": [{"name": "body", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "mode": 4}]}],
  "accessors": [
    {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
    {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
  ],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962},
    {"buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963}
  ],
  "buffers": [{"byteLength": 42, "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAAAAABAAIA"}]
}"#;

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fabr-import-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn glb(positions: [[f32; 3]; 3], indices: [u16; 3]) -> Vec<u8> {
        glb_nodes(positions, indices, r#"[{"name":"tri","mesh":0}]"#)
    }

    fn glb_nodes(positions: [[f32; 3]; 3], indices: [u16; 3], nodes: &str) -> Vec<u8> {
        let mut bin = Vec::new();
        for vertex in positions {
            for axis in vertex {
                bin.extend(axis.to_le_bytes());
            }
        }
        for index in indices {
            bin.extend(index.to_le_bytes());
        }
        let json = String::from(
            r#"{"asset":{"version":"2.0","generator":"glb-test"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":"#,
        ) + nodes
            + r#","meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]},{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":6}],"buffers":[{"byteLength":42}]}"#;
        let mut json_chunk = json.as_bytes().to_vec();
        while !json_chunk.len().is_multiple_of(4) {
            json_chunk.push(b' ');
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + json_chunk.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((json_chunk.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&json_chunk);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&bin);
        out
    }

    #[test]
    fn reads_counts_bounds_and_file_facts() {
        let path = temp("tri.gltf", TRI_GLTF.as_bytes());
        let report = import(&path, &ImportSettings::default()).expect("fixture imports");

        assert_eq!(report.file.version, "2.0");
        assert_eq!(report.file.generator.as_deref(), Some("unit-test"));
        assert!(!report.file.binary);
        assert_eq!(report.counts.nodes, 2);
        assert_eq!(report.counts.meshes, 1);
        assert_eq!(report.counts.vertices, 3);
        assert_eq!(report.counts.triangles, 1);

        let bounds = report.bounds.expect("fixture has geometry");
        assert_eq!(bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(bounds.max, [1.0, 1.0, 0.0]);
        assert_eq!(bounds.longest(), 1.0);
    }

    #[test]
    fn warns_about_duplicate_names_scale_normals_and_extensions() {
        let path = temp("warns.gltf", TRI_GLTF.as_bytes());
        let report = import(&path, &ImportSettings::default()).unwrap();
        let messages: Vec<&str> = report.warnings.iter().map(|w| w.message.as_str()).collect();

        assert!(messages.iter().any(|m| m.contains("2 nodes share this name")));
        assert!(messages.iter().any(|m| m.contains("non-uniform scale")));
        assert!(messages.iter().any(|m| m.contains("missing normals")));
        assert!(messages.iter().any(|m| m.contains("not supported")));
    }

    #[test]
    fn scale_and_up_axis_change_the_reported_bounds() {
        let path = temp("scaled.gltf", TRI_GLTF.as_bytes());
        let mut settings = ImportSettings::default();
        settings.transform.scale = 2.0;
        let report = import(&path, &settings).unwrap();
        assert_eq!(report.bounds.unwrap().max, [2.0, 2.0, 0.0]);

        settings.transform.up_axis = UpAxis::Z;
        let report = import(&path, &settings).unwrap();
        assert_eq!(report.bounds.unwrap().max[1], 0.0);
    }

    #[test]
    fn binary_gltf_is_detected_and_degenerate_triangles_are_reported() {
        let path = temp("flat.glb", &glb([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], [0, 1, 2]));
        let report = import(&path, &ImportSettings::default()).unwrap();

        assert!(report.file.binary);
        assert_eq!(report.counts.triangles, 1);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message.contains("degenerate triangles")),
            "a zero-area triangle must be warned about"
        );
    }

    #[test]
    fn bounds_apply_node_rotation_and_scale() {
        let nodes = r#"[{"name":"tri","mesh":0,"rotation":[0.0,0.70710678,0.0,0.70710678],"scale":[2.0,2.0,2.0]}]"#;
        let path = temp(
            "rotated.glb",
            &glb_nodes([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [0, 1, 2], nodes),
        );
        let report = import(&path, &ImportSettings::default()).unwrap();
        let bounds = report.bounds.expect("mesh has bounds");

        // 90 degrees about Y turns +X into -Z; both axes are then scaled by 2.
        assert!((bounds.min[2] + 2.0).abs() < 1e-4, "min {:?}", bounds.min);
        assert!((bounds.max[1] - 2.0).abs() < 1e-4, "max {:?}", bounds.max);
        assert!(bounds.max[0].abs() < 1e-4, "max {:?}", bounds.max);
        assert!(bounds.min[0].abs() < 1e-4, "min {:?}", bounds.min);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let path = temp("garbage.glb", b"this is not a model");
        let error = import(&path, &ImportSettings::default()).unwrap_err();
        assert!(matches!(error, ImportError::Parse(_)), "got {error:?}");
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let path = std::env::temp_dir().join("fabr-import-does-not-exist.gltf");
        let error = import(&path, &ImportSettings::default()).unwrap_err();
        assert!(matches!(error, ImportError::Io(_)), "got {error:?}");
    }
}
