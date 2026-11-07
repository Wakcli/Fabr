//! One-time conversion of legacy absolute node overrides to deltas.
//!
//! Scenes written during phase 5 stored the node's full local transform. New
//! edits store an offset so source changes still show through; loading an old
//! scene imports each model once, converts the values and leaves the file ready
//! to be saved in the delta form.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eme::prelude::*;
use eme_game_runtime::GuidIndex;
use eme_game_scene::{OverrideOp, SceneDoc, TransformDelta};

use crate::console::ConsoleLog;

use super::ImportSettings;

/// Converts every legacy absolute override in `doc` to a delta.
///
/// Returns the number of overrides rewritten. Overrides whose asset or node
/// cannot be resolved are left absolute so they still apply; the returned count
/// only covers successful conversions.
pub fn migrate_doc(
    doc: &mut SceneDoc,
    root: &Path,
    index: &GuidIndex,
    log: &mut ConsoleLog,
) -> usize {
    let mut migrated = 0;
    for obj in &mut doc.objects {
        if !obj
            .overrides
            .iter()
            .any(|entry| matches!(entry.op, OverrideOp::SetTransform(_)))
        {
            continue;
        }
        let Some(asset) = obj.asset.as_ref() else {
            continue;
        };
        let Some(model) = resolve(root, index, &asset.guid, &asset.path) else {
            log.push(format!(
                "could not migrate overrides for {}: model file is missing",
                obj.name
            ));
            continue;
        };
        let report = match super::import(&model, &ImportSettings::default()) {
            Ok(report) => report,
            Err(err) => {
                log.push(format!(
                    "could not migrate overrides for {}: {err}",
                    obj.name
                ));
                continue;
            }
        };
        let bases: HashMap<_, _> = report
            .nodes
            .iter()
            .map(|node| (node.key.clone(), node.transform))
            .collect();

        for entry in &mut obj.overrides {
            let OverrideOp::SetTransform(absolute) = entry.op else {
                continue;
            };
            let Some(base) = bases.get(&entry.key).copied() else {
                continue;
            };
            entry.op = OverrideOp::SetTransformDelta(TransformDelta::from_absolute(absolute, base));
            migrated += 1;
        }
    }
    if migrated > 0 {
        log.push(format!(
            "migrated {migrated} absolute node override(s) to deltas"
        ));
        info!("migrated {migrated} absolute node override(s) to deltas");
    }
    migrated
}

fn resolve(root: &Path, index: &GuidIndex, guid: &str, path: &str) -> Option<PathBuf> {
    index
        .get(guid)
        .map(Path::to_path_buf)
        .filter(|path| path.is_file())
        .or_else(|| {
            let candidate = root.join(path);
            candidate.is_file().then_some(candidate)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eme_game_scene::{ObjectKind, ObjectTransform, OverrideOp};

    const TRI_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"name":"Seat","translation":[0.0,2.0,0.0]}],"meshes":[],"accessors":[],"bufferViews":[],"buffers":[]}"#;

    #[test]
    fn a_legacy_absolute_override_becomes_a_delta() {
        let dir = std::env::temp_dir().join(format!("fabr-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("chair.gltf");
        std::fs::write(&model, TRI_GLTF).unwrap();
        std::fs::write(
            model.with_file_name("chair.gltf.import.ron"),
            "(version: 1, guid: \"migrate-guid\", settings: ())",
        )
        .unwrap();

        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "chair",
            ObjectTransform::from_translation([0.0; 3]),
        );
        doc.set_asset(
            id,
            Some(eme_game_scene::AssetRef {
                guid: "migrate-guid".into(),
                path: "chair.gltf".into(),
            }),
        );
        doc.set_override(
            id,
            eme_game_scene::NodeKey {
                path: vec![0],
                name: "Seat".into(),
            },
            OverrideOp::SetTransform(ObjectTransform::from_translation([0.0, 5.0, 0.0])),
        );

        let index = GuidIndex::scan(&dir);
        let mut log = ConsoleLog::default();
        let count = migrate_doc(&mut doc, &dir, &index, &mut log);
        assert_eq!(count, 1);
        let entry = &doc.overrides(id)[0];
        let OverrideOp::SetTransformDelta(delta) = entry.op else {
            panic!("expected a delta, got {:?}", entry.op);
        };
        assert_eq!(delta.translation, [0.0, 3.0, 0.0]);
        assert!(log.lines().iter().any(|line| line.contains("migrated")));
    }

    #[test]
    fn a_scene_without_legacy_ops_is_untouched() {
        let mut doc = SceneDoc::new();
        let id = doc.add(
            ObjectKind::Model,
            "chair",
            ObjectTransform::from_translation([0.0; 3]),
        );
        doc.set_override(
            id,
            eme_game_scene::NodeKey {
                path: vec![0],
                name: "Seat".into(),
            },
            OverrideOp::SetVisible { visible: false },
        );
        let mut log = ConsoleLog::default();
        assert_eq!(
            migrate_doc(
                &mut doc,
                Path::new("/nowhere"),
                &GuidIndex::default(),
                &mut log
            ),
            0
        );
        assert!(log.lines().is_empty());
    }
}
