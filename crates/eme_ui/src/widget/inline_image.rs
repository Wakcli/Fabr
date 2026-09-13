use eme_asset::asset_changed::AssetChanged;
use eme_asset::AsAssetId;
use eme_asset::Assets;
use eme_asset::Handle;
use eme_color::Color;
use eme_ecs::change_detection::DetectChangesMut;
use eme_ecs::component::Component;
use eme_ecs::query::Changed;
use eme_ecs::query::Or;
use eme_ecs::reflect::ReflectComponent;
use eme_ecs::system::Query;
use eme_ecs::system::Res;
use eme_image::Image;
use eme_image::TRANSPARENT_IMAGE_HANDLE;
use eme_math::Vec2;
use eme_reflect::std_traits::ReflectDefault;
use eme_reflect::Reflect;
use eme_text::InlineBox;

/// An inline image
#[derive(Component, Debug, Clone, Reflect, PartialEq)]
#[reflect(Component, Default, Debug, PartialEq, Clone)]
#[require(InlineBox)]
pub struct InlineImage {
    /// The tint color used to draw the image.
    ///
    /// This is multiplied by the color of each pixel in the image.
    /// The field value defaults to solid white, which will pass the image through unmodified.
    pub color: Color,
    /// Handle to the texture.
    ///
    /// This defaults to a [`TRANSPARENT_IMAGE_HANDLE`], which points to a fully transparent 1x1 texture.
    pub image: Handle<Image>,
}

impl Default for InlineImage {
    /// A transparent 1x1 image with a solid white tint.
    fn default() -> Self {
        InlineImage {
            color: Color::WHITE,
            image: TRANSPARENT_IMAGE_HANDLE,
        }
    }
}

impl AsAssetId for InlineImage {
    type Asset = Image;

    fn as_asset_id(&self) -> eme_asset::AssetId<Self::Asset> {
        self.image.id()
    }
}

/// For each `InlineImage` update the size of its `InlineBox` with its image size, if it changed.
pub fn update_inline_image_boxes(
    image_assets: Res<Assets<Image>>,
    mut query: Query<
        (&InlineImage, &mut InlineBox),
        Or<(Changed<InlineImage>, AssetChanged<InlineImage>)>,
    >,
) {
    for (inline_image, mut inline_box) in &mut query {
        if let Some(image_asset) = image_assets.get(&inline_image.image) {
            inline_box.set_if_neq(InlineBox {
                kind: eme_text::InlineBoxKind::InFlow,
                size: image_asset.size().as_vec2(),
            });
        } else {
            inline_box.set_if_neq(InlineBox {
                kind: eme_text::InlineBoxKind::OutOfFlow,
                size: Vec2::ZERO,
            });
        }
    }
}
