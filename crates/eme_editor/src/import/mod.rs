//! glTF and GLB import.
//!
//! One [`ImportSettings`] describes the options, [`import`] is the only parser,
//! and every call returns one [`ImportReport`] or one [`ImportError`]. The UI
//! reaches it through [`ImportService`], which runs the parse off the main thread.

mod importer;
mod panel;
mod report;
mod sidecar;
mod service;
mod settings;

pub use importer::import;
pub use panel::{OptionsPanel, panel, sync as sync_panel};
pub use sidecar::Sidecar;
pub use report::{ImportError, ImportReport};
pub use service::{ImportRequest, ImportService, ImportStatus, poll_system, status_system};
pub use settings::ImportSettings;
