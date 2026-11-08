//! Editor extension surface shared by the Fabr editor and native plugins.
//!
//! A plugin registers into [`EditorExtensions`], a resource the editor owns.
//! Everything here is value-based and data-driven: plugins describe panels,
//! inspector fields and importers, and the editor renders them with its own UI.
//! That keeps the editor's layout and theme out of the plugin ABI while still
//! letting plugins add real, live functionality.
//!
//! ```ignore
//! let extensions = app
//!     .world_mut()
//!     .get_resource_mut::<EditorExtensions>()
//!     .expect("the editor provides EditorExtensions");
//! extensions.add_panel(
//!     PanelSpec::new("sample", "Sample", |world| {
//!         let elapsed = world.resource::<Time>().elapsed_secs();
//!         vec![PanelRow::text("Elapsed", format!("{elapsed:.1} s"))]
//!     })
//!     .with_button("Reset", |world| { /* ... */ }),
//! );
//! ```

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use eme_ecs::prelude::*;

/// Registry of editor extensions; initialized by the editor before plugins load.
#[derive(Resource, Default)]
pub struct EditorExtensions {
    panels: Vec<PanelSpec>,
    fields: Vec<Arc<dyn InspectorField>>,
    importers: Vec<Arc<dyn ImporterExtension>>,
}

impl EditorExtensions {
    /// Registers one panel. Returns false when the id is already taken.
    pub fn add_panel(&mut self, panel: PanelSpec) -> bool {
        if self.panels.iter().any(|known| known.id() == panel.id()) {
            return false;
        }
        self.panels.push(panel);
        true
    }

    /// Registers one Inspector field. Returns false when the id is already taken.
    pub fn add_field(&mut self, field: Arc<dyn InspectorField>) -> bool {
        if self.fields.iter().any(|known| known.id() == field.id()) {
            return false;
        }
        self.fields.push(field);
        true
    }

    /// Registers one importer. Returns false when the id is already taken.
    pub fn add_importer(&mut self, importer: Arc<dyn ImporterExtension>) -> bool {
        if self.importers.iter().any(|known| known.id() == importer.id()) {
            return false;
        }
        self.importers.push(importer);
        true
    }

    /// Panels in registration order.
    pub fn panels(&self) -> &[PanelSpec] {
        &self.panels
    }

    /// Fields in registration order.
    pub fn fields(&self) -> &[Arc<dyn InspectorField>] {
        &self.fields
    }

    /// Importers in registration order.
    pub fn importers(&self) -> &[Arc<dyn ImporterExtension>] {
        &self.importers
    }

    /// Whether any extension is registered.
    pub fn is_empty(&self) -> bool {
        self.panels.is_empty() && self.fields.is_empty() && self.importers.is_empty()
    }

    /// The importer that handles `extension`, ignoring case and a leading dot.
    pub fn importer_for(&self, extension: &str) -> Option<Arc<dyn ImporterExtension>> {
        let wanted = extension.trim_start_matches('.').to_ascii_lowercase();
        self.importers
            .iter()
            .find(|importer| {
                importer
                    .extensions()
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&wanted))
            })
            .cloned()
    }
}

/// One panel a plugin contributes to the editor's Extensions dock.
#[derive(Clone)]
pub struct PanelSpec {
    id: String,
    title: String,
    rows: Arc<dyn Fn(&mut World) -> Vec<PanelRow> + Send + Sync>,
    buttons: Vec<PanelButton>,
}

impl PanelSpec {
    /// Builds a panel from its id, title and a live row provider.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        rows: impl Fn(&mut World) -> Vec<PanelRow> + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            rows: Arc::new(rows),
            buttons: Vec::new(),
        }
    }

    /// Adds a button below the rows.
    pub fn with_button(
        mut self,
        label: impl Into<String>,
        run: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> Self {
        self.buttons.push(PanelButton {
            label: label.into(),
            run: Arc::new(run),
        });
        self
    }

    /// Stable panel id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Title shown above the rows.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Evaluates the rows for the current frame.
    pub fn rows(&self, world: &mut World) -> Vec<PanelRow> {
        (self.rows)(world)
    }

    /// Buttons below the rows.
    pub fn buttons(&self) -> &[PanelButton] {
        &self.buttons
    }
}

/// One label/value line in a plugin panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRow {
    /// Row label.
    pub label: String,
    /// Formatted value.
    pub value: String,
    /// How the value should read.
    pub tone: PanelTone,
}

/// Emphasis the editor gives a panel value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelTone {
    /// Normal text.
    #[default]
    Text,
    /// A healthy or successful value.
    Ok,
    /// A value that needs attention.
    Warn,
}

impl PanelRow {
    /// A plain row.
    pub fn text(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone: PanelTone::Text,
        }
    }

    /// A row rendered as healthy.
    pub fn ok(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone: PanelTone::Ok,
        }
    }

    /// A row rendered as a warning.
    pub fn warn(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            tone: PanelTone::Warn,
        }
    }
}

/// A button in a plugin panel.
#[derive(Clone)]
pub struct PanelButton {
    label: String,
    run: Arc<dyn Fn(&mut World) + Send + Sync>,
}

impl PanelButton {
    /// Button label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Runs the button's action.
    pub fn run(&self, world: &mut World) {
        (self.run)(world);
    }
}

/// One editable Inspector row a plugin contributes for scene objects.
pub trait InspectorField: Send + Sync {
    /// Stable id; duplicates are rejected.
    fn id(&self) -> &str;

    /// Label shown in the Inspector.
    fn label(&self) -> &str;

    /// Whether this field applies to the object with `object` id and `kind` name.
    fn applies(&self, object: u64, kind: &str) -> bool;

    /// Current value, or `None` to show an empty field.
    fn value(&self, world: &mut World, object: u64) -> Option<String>;

    /// Applies an edited value; the returned text is what the editor reports.
    fn apply(&self, world: &mut World, object: u64, text: &str) -> Result<String, String>;
}

/// An importer a plugin contributes to the editor.
///
/// The editor invokes it when the user activates a file whose extension the
/// importer claims. The importer reads `source` and writes whatever artifact it
/// produces under `project`, returning the artifact path relative to the project.
/// A returned `.ember` path is opened as a scene.
pub trait ImporterExtension: Send + Sync {
    /// Stable id; duplicates are rejected.
    fn id(&self) -> &str;

    /// File extensions handled, without the dot.
    fn extensions(&self) -> &[&str];

    /// Converts `source` into a project artifact.
    ///
    /// Both paths are UTF-8; the returned path is relative to the project.
    fn import(&self, source: &str, project: &str) -> Result<String, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use alloc::vec;

    fn world() -> World {
        World::new()
    }

    #[test]
    fn panels_read_live_values_and_run_buttons() {
        #[derive(Resource, Default)]
        struct Clicks(u32);

        let mut extensions = EditorExtensions::default();
        assert!(extensions.add_panel(
            PanelSpec::new("sample", "Sample", |world| {
                let clicks = world.resource::<Clicks>().0;
                vec![PanelRow::text("Clicks", clicks.to_string())]
            })
            .with_button("Add", |world| world.resource_mut::<Clicks>().0 += 1)
        ));
        assert!(!extensions.add_panel(PanelSpec::new("sample", "Other", |_| Vec::new())));

        let mut world = world();
        world.insert_resource(Clicks::default());
        let panel = &extensions.panels()[0];
        assert_eq!(panel.rows(&mut world)[0].value, "0");
        panel.buttons()[0].run(&mut world);
        assert_eq!(panel.rows(&mut world)[0].value, "1");
    }

    #[test]
    fn inspector_fields_are_registered_and_deduplicated() {
        struct Note;
        impl InspectorField for Note {
            fn id(&self) -> &str {
                "note"
            }
            fn label(&self) -> &str {
                "Note"
            }
            fn applies(&self, _object: u64, kind: &str) -> bool {
                kind == "Cube"
            }
            fn value(&self, _world: &mut World, object: u64) -> Option<String> {
                Some(format!("object {object}"))
            }
            fn apply(&self, _world: &mut World, object: u64, text: &str) -> Result<String, String> {
                Ok(format!("{object}={text}"))
            }
        }

        let mut extensions = EditorExtensions::default();
        assert!(extensions.add_field(Arc::new(Note)));
        assert!(!extensions.add_field(Arc::new(Note)));
        let field = &extensions.fields()[0];
        assert!(field.applies(7, "Cube"));
        assert!(!field.applies(7, "Sun"));
        assert_eq!(field.value(&mut World::new(), 7).as_deref(), Some("object 7"));
        assert_eq!(field.apply(&mut World::new(), 7, "x").unwrap(), "7=x");
    }

    #[test]
    fn importers_are_found_by_extension() {
        struct Text;
        impl ImporterExtension for Text {
            fn id(&self) -> &str {
                "text"
            }
            fn extensions(&self) -> &[&str] {
                &["txt", "md"]
            }
            fn import(&self, source: &str, project: &str) -> Result<String, String> {
                let name = source.rsplit('/').next().unwrap_or(source);
                Ok(format!("{project}/{name}"))
            }
        }

        let mut extensions = EditorExtensions::default();
        assert!(extensions.add_importer(Arc::new(Text)));
        assert!(!extensions.add_importer(Arc::new(Text)));
        assert!(extensions.importer_for("TXT").is_some());
        assert!(extensions.importer_for(".md").is_some());
        assert!(extensions.importer_for("lua").is_none());
        assert!(!extensions.is_empty());
    }
}
