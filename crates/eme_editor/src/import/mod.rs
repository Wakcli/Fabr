//! glTF and GLB import.
//!
//! One [`ImportSettings`] describes the options, [`import`] is the only parser,
//! and every call returns one [`ImportReport`] or one [`ImportError`].

mod importer;
mod report;
mod settings;

#[expect(
    unused_imports,
    reason = "phase 2 entry points consume the importer API"
)]
pub use importer::{SUPPORTED_EXTENSIONS, import};
#[expect(
    unused_imports,
    reason = "phase 2 entry points consume the report API"
)]
pub use report::{
    AnimationInfo, Bounds, Counts, FileInfo, ImportError, ImportReport, ImportWarning, Severity,
    StageTimings, TextureInfo,
};
#[expect(
    unused_imports,
    reason = "phase 2 entry points consume the settings API"
)]
pub use settings::{
    AnimationSettings, GeometrySettings, ImportSettings, MaterialSettings, OutputSettings,
    SceneSettings, TransformSettings, UpAxis,
};
