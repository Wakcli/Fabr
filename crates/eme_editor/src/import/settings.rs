//! Options for importing a glTF or GLB asset.
//!
//! Every entry point (drop, menu, inspector retry) builds one of these and hands
//! it to [`super::import`], so a setting only has to be honoured in one place.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Axis the source file treats as up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpAxis {
    /// glTF's own convention.
    #[default]
    Y,
    /// Blender's default export when not converted.
    Z,
}

/// Transform applied while converting the file into the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransformSettings {
    /// Uniform scale applied to everything, in metres per source unit.
    pub scale: f32,
    /// Which source axis becomes world up.
    pub up_axis: UpAxis,
}

impl Default for TransformSettings {
    fn default() -> Self {
        Self {
            scale: 1.0,
            up_axis: UpAxis::Y,
        }
    }
}

/// Mesh data to keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeometrySettings {
    /// Keep normal attributes.
    pub normals: bool,
    /// Keep tangent attributes.
    pub tangents: bool,
    /// Collapse primitives into a single mesh per node.
    pub merge_meshes: bool,
    /// Generate collision shapes.
    pub colliders: bool,
}

impl Default for GeometrySettings {
    fn default() -> Self {
        Self {
            normals: true,
            tangents: true,
            merge_meshes: false,
            colliders: false,
        }
    }
}

/// Material and texture handling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialSettings {
    /// Import materials at all.
    pub import: bool,
    /// Write material files next to the asset instead of embedding them.
    pub extract: bool,
    /// Keep overrides made in the editor when the asset is re-imported.
    pub keep_on_reimport: bool,
    /// Largest texture edge allowed, in pixels.
    pub max_texture_size: u32,
    /// Generate mipmaps.
    pub mipmaps: bool,
}

impl Default for MaterialSettings {
    fn default() -> Self {
        Self {
            import: true,
            extract: false,
            keep_on_reimport: true,
            max_texture_size: 4096,
            mipmaps: true,
        }
    }
}

/// Animation clips.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationSettings {
    /// Import animation data.
    pub import: bool,
    /// One clip per animation instead of a single track.
    pub split_clips: bool,
    /// Loop imported clips.
    pub looping: bool,
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            import: true,
            split_clips: false,
            looping: true,
        }
    }
}

/// Extra scene data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneSettings {
    /// Import cameras.
    pub cameras: bool,
    /// Import punctual lights.
    pub lights: bool,
    /// Import `extras` metadata.
    pub extras: bool,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            cameras: true,
            lights: true,
            extras: false,
        }
    }
}

/// Where the imported asset lands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    /// Folder the asset is copied into, relative to the project root.
    pub folder: PathBuf,
    /// Place an instance in the open scene after importing.
    pub instance_after_import: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            folder: PathBuf::from("assets"),
            instance_after_import: true,
        }
    }
}

/// All import options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportSettings {
    /// Transform conversion.
    pub transform: TransformSettings,
    /// Mesh data.
    pub geometry: GeometrySettings,
    /// Materials and textures.
    pub materials: MaterialSettings,
    /// Animation data.
    pub animations: AnimationSettings,
    /// Cameras, lights and extras.
    pub scene: SceneSettings,
    /// Destination folder and instancing.
    pub output: OutputSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let settings = ImportSettings::default();
        assert_eq!(settings.transform.scale, 1.0);
        assert_eq!(settings.transform.up_axis, UpAxis::Y);
        assert!(settings.geometry.normals);
        assert_eq!(settings.materials.max_texture_size, 4096);
        assert_eq!(settings.output.folder, PathBuf::from("assets"));
        assert!(settings.output.instance_after_import);
    }

    #[test]
    fn round_trips_through_ron() {
        let mut settings = ImportSettings::default();
        settings.transform.scale = 0.01;
        settings.geometry.colliders = true;
        settings.materials.extract = true;

        let text = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default()).unwrap();
        let back: ImportSettings = ron::from_str(&text).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let back: ImportSettings = ron::from_str("(transform: (scale: 2.0))").unwrap();
        assert_eq!(back.transform.scale, 2.0);
        assert_eq!(back.transform.up_axis, UpAxis::Y);
        assert_eq!(back.materials.max_texture_size, 4096);
    }
}
