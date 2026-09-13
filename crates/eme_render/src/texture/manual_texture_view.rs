use eme_camera::ManualTextureViewHandle;
use eme_ecs::resource::Resource;
use eme_extract_macros::ExtractResource;
use eme_math::UVec2;
use eme_platform::collections::HashMap;
use wgpu::TextureFormat;

use crate::{render_resource::TextureView, RenderApp};

/// A manually managed [`TextureView`] for use as a [`eme_camera::RenderTarget`].
#[derive(Debug, Clone)]
pub struct ManualTextureView {
    pub texture_view: TextureView,
    pub size: UVec2,
    pub view_format: TextureFormat,
}

impl ManualTextureView {
    pub fn with_default_format(texture_view: TextureView, size: UVec2) -> Self {
        Self {
            texture_view,
            size,
            view_format: TextureFormat::Rgba8UnormSrgb,
        }
    }
}

/// Resource that stores manually managed [`ManualTextureView`]s for use as a [`RenderTarget`](eme_camera::RenderTarget).
/// This type dereferences to a `HashMap<ManualTextureViewHandle, ManualTextureView>`.
/// To add a new texture view, pick a new [`ManualTextureViewHandle`] and insert it into the map.
/// Then, to render to the view, set a [`Camera`](eme_camera::Camera)s `target` to `RenderTarget::TextureView(handle)`.
/// ```ignore
/// # use eme_ecs::prelude::*;
/// # let mut world = World::default();
/// # world.insert_resource(ManualTextureViews::default());
/// # let texture_view = todo!();
/// let manual_views = world.resource_mut::<ManualTextureViews>();
/// let manual_view = ManualTextureView::with_default_format(texture_view, UVec2::new(1024, 1024));
///
/// // Choose an unused handle value; it's likely only you are inserting manual views.
/// const MANUAL_VIEW_HANDLE: ManualTextureViewHandle = ManualTextureViewHandle::new(42);
/// manual_views.insert(MANUAL_VIEW_HANDLE, manual_view);
///
/// // Now you can spawn a Camera that renders to the manual view:
/// # use eme_camera::{Camera, RenderTarget};
/// world.spawn(Camera {
///     target: RenderTarget::TextureView(MANUAL_VIEW_HANDLE),
///     ..Default::default()
/// });
/// ```
/// Eme will then use the `ManualTextureViews` resource to find your texture view and render to it.
#[derive(Default, Clone, Resource, ExtractResource)]
#[extract_app(RenderApp)]
pub struct ManualTextureViews(HashMap<ManualTextureViewHandle, ManualTextureView>);

impl core::ops::Deref for ManualTextureViews {
    type Target = HashMap<ManualTextureViewHandle, ManualTextureView>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for ManualTextureViews {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
