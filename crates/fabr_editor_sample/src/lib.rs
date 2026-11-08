#![allow(unsafe_code, reason = "the plugin entry point is unsafe by ABI")]

//! Example editor extension plugin.
//!
//! Registers the three extension points on a live editor:
//!
//! - a panel with live rows and a button,
//! - an Inspector field storing a note per scene object,
//! - an importer turning a `.cube` blockout list into a scene document.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use std::ffi::c_void;
use std::path::Path;

extern crate alloc;

use fabr::editor::{EditorExtensions, ImporterExtension, InspectorField, PanelRow, PanelSpec};
use fabr::prelude::*;
use eme_game_scene::{ObjectKind, ObjectTransform, SceneDoc};

/// State the sample keeps between frames.
#[derive(Resource, Default)]
pub struct SampleState {
    /// Number of times the panel button was pressed.
    pub clicks: u32,
    /// Notes written through the Inspector field, per object id.
    pub notes: BTreeMap<u64, String>,
    /// Last imported artifact, for the panel row.
    pub last_import: String,
}

/// A panel with live values and a button.
#[derive(Clone, Copy, Default)]
pub struct SamplePanel;

impl SamplePanel {
    /// Builds the panel spec.
    pub fn spec() -> PanelSpec {
        PanelSpec::new("sample.panel", "Sample Plugin", |world| {
            let state = world.resource::<SampleState>();
            let elapsed = world.resource::<Time>().elapsed_secs();
            vec![
                PanelRow::ok("Plugin", "loaded"),
                PanelRow::text("Elapsed", format!("{elapsed:.1} s")),
                PanelRow::text("Clicks", state.clicks.to_string()),
                PanelRow::text("Notes", state.notes.len().to_string()),
                PanelRow::text(
                    "Last import",
                    if state.last_import.is_empty() {
                        "none".to_owned()
                    } else {
                        state.last_import.clone()
                    },
                ),
            ]
        })
        .with_button("Press me", |world| {
            world.resource_mut::<SampleState>().clicks += 1;
        })
    }
}

/// Inspector row that stores a free-form note for the selected object.
#[derive(Clone, Copy, Default)]
pub struct SampleNote;

impl InspectorField for SampleNote {
    fn id(&self) -> &str {
        "sample.note"
    }

    fn label(&self) -> &str {
        "Sample note"
    }

    fn applies(&self, _object: u64, _kind: &str) -> bool {
        true
    }

    fn value(&self, world: &mut World, object: u64) -> Option<String> {
        world.resource::<SampleState>().notes.get(&object).cloned()
    }

    fn apply(&self, world: &mut World, object: u64, text: &str) -> Result<String, String> {
        let text = text.trim();
        let mut state = world.resource_mut::<SampleState>();
        if text.is_empty() {
            state.notes.remove(&object);
            return Ok("cleared".to_owned());
        }
        state.notes.insert(object, text.to_owned());
        Ok(format!("saved {}", state.notes.len()))
    }
}

/// Importer for `.cube` blockout lists.
///
/// Each line is `x y z size`; `#` starts a comment. The importer writes
/// `scenes/<name>.ember`, which the editor opens as a scene.
#[derive(Clone, Copy, Default)]
pub struct CubeImporter;

impl ImporterExtension for CubeImporter {
    fn id(&self) -> &str {
        "sample.cube"
    }

    fn extensions(&self) -> &[&str] {
        &["cube"]
    }

    fn import(&self, source: &str, project: &str) -> Result<String, String> {
        let source = Path::new(source);
        let project = Path::new(project);
        let text = std::fs::read_to_string(source)
            .map_err(|err| format!("{}: {err}", source.display()))?;
        let stem = source
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "blocks".to_owned());
        let mut doc = SceneDoc::new();
        let mut count = 0;
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let values: Vec<&str> = line.split_whitespace().collect();
            let parsed: Option<Vec<f32>> = values.iter().map(|value| value.parse().ok()).collect();
            let Some(values) = parsed.filter(|values| values.len() == 4) else {
                return Err(format!(
                    "{}:{}: expected `x y z size`",
                    source.display(),
                    index + 1
                ));
            };
            let size = values[3].abs().max(0.05);
            doc.add(
                ObjectKind::Cube,
                format!("{stem} {}", count + 1),
                ObjectTransform {
                    translation: [values[0], values[1], values[2]],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [size, size, size],
                },
            );
            count += 1;
        }
        if count == 0 {
            return Err(format!("{}: no blocks found", source.display()));
        }
        let ron = doc.to_ron().map_err(|err| err.to_string())?;
        let destination = project.join("scenes").join(format!("{stem}.ember"));
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        std::fs::write(&destination, ron)
            .map_err(|err| format!("{}: {err}", destination.display()))?;
        Ok(format!("scenes/{stem}.ember"))
    }
}

/// Registers the extension points when the editor loads this plugin.
#[unsafe(no_mangle)]
pub extern "C" fn fabr_plugin_abi() -> u32 {
    fabr::PLUGIN_ABI_VERSION
}

/// # Safety
/// `app` must point at the host's live `App`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fabr_plugin_register(app: *mut c_void) {
    // SAFETY: the host passes its live App pointer, built against the same
    // engine dylib and plugin ABI this library was compiled with.
    let app = unsafe { &mut *app.cast::<App>() };
    app.insert_resource(SampleState::default());
    let Some(mut extensions) = app.world_mut().get_resource_mut::<EditorExtensions>() else {
        return;
    };
    extensions.add_panel(SamplePanel::spec());
    extensions.add_field(Arc::new(SampleNote));
    extensions.add_importer(Arc::new(CubeImporter));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fabr-editor-sample-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_panel_reads_state_and_the_button_changes_it() {
        let mut world = World::new();
        world.insert_resource(SampleState::default());
        world.insert_resource(Time::<()>::default());
        let spec = SamplePanel::spec();
        let rows = spec.rows(&mut world);
        assert_eq!(rows[0].value, "loaded");
        assert_eq!(rows[2].value, "0");
        assert_eq!(rows[4].value, "none");
        spec.buttons()[0].run(&mut world);
        spec.buttons()[0].run(&mut world);
        assert_eq!(spec.rows(&mut world)[2].value, "2");
    }

    #[test]
    fn the_inspector_field_stores_and_clears_a_note() {
        let mut world = World::new();
        world.insert_resource(SampleState::default());
        let field = SampleNote;
        assert!(field.value(&mut world, 4).is_none());
        assert_eq!(field.apply(&mut world, 4, " hello ").unwrap(), "saved 1");
        assert_eq!(field.value(&mut world, 4).as_deref(), Some("hello"));
        assert_eq!(field.apply(&mut world, 4, "  ").unwrap(), "cleared");
        assert!(field.value(&mut world, 4).is_none());
    }

    #[test]
    fn the_cube_importer_writes_a_scene() {
        let project = scratch("project");
        let source = project.join("blocks.cube");
        let mut file = std::fs::File::create(&source).unwrap();
        writeln!(file, "# a two block floor").unwrap();
        writeln!(file, "0 0.5 0 1").unwrap();
        writeln!(file, "2 0.5 0 1.5").unwrap();
        drop(file);

        let artifact = CubeImporter
            .import(&source.to_string_lossy(), &project.to_string_lossy())
            .unwrap();
        assert_eq!(artifact, "scenes/blocks.ember");
        let text = std::fs::read_to_string(project.join(&artifact)).unwrap();
        let doc = SceneDoc::from_ron(&text).unwrap();
        assert_eq!(doc.len(), 2);
        assert_eq!(doc.objects[1].scale, [1.5, 1.5, 1.5]);

        std::fs::write(&source, "# nothing\n").unwrap();
        assert!(
            CubeImporter
                .import(&source.to_string_lossy(), &project.to_string_lossy())
                .is_err()
        );
        std::fs::write(&source, "0 0 0\n").unwrap();
        assert!(
            CubeImporter
                .import(&source.to_string_lossy(), &project.to_string_lossy())
                .is_err()
        );
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn all_three_extension_points_register() {
        let mut extensions = EditorExtensions::default();
        assert!(extensions.add_panel(SamplePanel::spec()));
        assert!(extensions.add_field(Arc::new(SampleNote)));
        assert!(extensions.add_importer(Arc::new(CubeImporter)));
        assert!(extensions.importer_for("CUBE").is_some());
        assert!(!extensions.is_empty());
    }
}
