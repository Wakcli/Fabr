//! This module is for 'retained' alternatives to the 'immediate mode' [`Gizmos`](crate::gizmos::Gizmos) system parameter.

use core::ops::{Deref, DerefMut};

use eme_asset::Handle;
use eme_ecs::{component::Component, reflect::ReflectComponent, template::FromTemplate};
use eme_reflect::{std_traits::ReflectDefault, Reflect};
use eme_transform::components::Transform;

use crate::{
    config::{ErasedGizmoConfigGroup, GizmoLineConfig},
    gizmos::GizmoBuffer,
    GizmoAsset,
};

impl Deref for GizmoAsset {
    type Target = GizmoBuffer<ErasedGizmoConfigGroup, ()>;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

impl DerefMut for GizmoAsset {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buffer
    }
}

/// A component that draws the gizmos of a [`GizmoAsset`].
///
/// When drawing a greater number of static lines a [`Gizmo`] component can
/// have far better performance than the [`Gizmos`] system parameter,
/// but the system parameter will perform better for smaller lines that update often.
///
/// ## Example
/// ```
/// # use eme_ecs::prelude::*;
/// # use eme_gizmos::prelude::*;
/// # use eme_asset::prelude::*;
/// # use eme_color::palettes::css::*;
/// # use eme_utils::default;
/// # use eme_math::prelude::*;
/// fn system(
///     mut commands: Commands,
///     mut gizmo_assets: ResMut<Assets<GizmoAsset>>,
/// ) {
///     let mut gizmo = GizmoAsset::default();
///
///     gizmo.sphere(Vec3::ZERO, 1., RED);
///
///     commands.spawn(Gizmo {
///         handle: gizmo_assets.add(gizmo),
///         line_config: GizmoLineConfig {
///             width: 4.,
///             ..default()
///         },
///         ..default()
///     });
/// }
/// ```
///
/// [`Gizmos`]: crate::gizmos::Gizmos
#[derive(Component, Clone, Debug, Default, Reflect, FromTemplate)]
#[reflect(Component, Clone, Default)]
#[require(Transform)]
pub struct Gizmo {
    /// The handle to the gizmo to draw.
    pub handle: Handle<GizmoAsset>,
    /// The line specific configuration for this gizmo.
    pub line_config: GizmoLineConfig,
    /// How closer to the camera than real geometry the gizmo should be.
    ///
    /// In 2D this setting has no effect and is effectively always -1.
    ///
    /// Value between -1 and 1 (inclusive).
    /// * 0 means that there is no change to the gizmo position when rendering
    /// * 1 means it is furthest away from camera as possible
    /// * -1 means that it will always render in front of other things.
    ///
    /// This is typically useful if you are drawing wireframes on top of polygons
    /// and your wireframe is z-fighting (flickering on/off) with your main model.
    /// You would set this value to a negative number close to 0.
    pub depth_bias: f32,
}
