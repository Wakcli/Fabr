//! glTF and GLB import.
//!
//! One [`ImportSettings`] describes the options, [`import`] is the only parser,
//! and every call returns one [`ImportReport`] or one [`ImportError`]. The UI
//! reaches it through [`ImportService`], which runs the parse off the main thread.

mod diff;
mod importer;
mod migrate;
mod panel;
mod report;
mod sidecar;
mod service;
mod settings;
mod watch;

pub use diff::NodeDiff;
pub use importer::import;
pub use migrate::migrate_doc;
pub use panel::{OptionsPanel, panel, sync as sync_panel};
pub use sidecar::Sidecar;
pub use report::{ImportError, ImportReport};
pub use service::{ImportRequest, ImportService, ImportStatus, poll_system, status_system};
pub use settings::ImportSettings;
pub use watch::{apply_reloads, watch_models, ModelWatcher};
