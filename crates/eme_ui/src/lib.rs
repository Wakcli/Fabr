#![expect(missing_docs, reason = "Not all docs are written yet, see #3492.")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://eme.org/assets/icon.png",
    html_favicon_url = "https://eme.org/assets/icon.png"
)]

//! This crate contains Eme's UI system, which can be used to create UI for both 2D and 3D games
//! # Basic usage
//! Spawn UI elements with [`widget::Button`], [`ImageNode`](widget::ImageNode), [`Text`](prelude::Text) and [`Node`]
//! This UI is laid out with the Flexbox and CSS Grid layout models (see <https://cssreference.io/flexbox/>)

extern crate alloc;

pub mod auto_directional_navigation;
pub mod interaction_states;
pub mod measurement;
pub mod update;
pub mod widget;

pub mod gradients;
#[cfg(feature = "eme_picking")]
pub mod picking_backend;
pub mod ui_transform;

use eme_derive::{Deref, DerefMut};
#[cfg(feature = "eme_picking")]
use eme_picking::PickingSystems;
use eme_reflect::{std_traits::ReflectDefault, Reflect};
mod accessibility;
pub use accessibility::AccessibilityUiSystems;
// This module is not re-exported, but is instead made public.
// This is intended to discourage accidental use of the experimental API.
pub mod experimental;
mod focus;
mod geometry;
mod layout;
mod stack;
mod ui_node;

use eme_text::{detect_text_needs_rerender, EditableTextSystems};
pub use focus::*;
pub use geometry::*;
pub use gradients::*;
pub use interaction_states::{
    Checkable, Checked, InteractionDisabled, Pressed, Selectable, Selected,
};
pub use layout::*;
pub use measurement::*;
pub use ui_node::*;
pub use ui_transform::*;
pub use widget::TextNodeFlags;

/// The UI prelude.
///
/// This includes the most common types in this crate, re-exported for your convenience.
pub mod prelude {
    pub use crate::accessibility::AccessibleLabel;
    #[doc(hidden)]
    #[cfg(feature = "eme_picking")]
    pub use crate::picking_backend::{UiPickingCamera, UiPickingPlugin, UiPickingSettings};
    #[doc(hidden)]
    pub use crate::widget::{Text, TextShadow, TextUiReader, TextUiWriter};
    #[expect(
        deprecated,
        reason = "Should be removed after 0.20 is released when Button & Interaction are removed."
    )]
    #[doc(hidden)]
    pub use crate::{widget::Button, Interaction};
    #[doc(hidden)]
    pub use {
        crate::{
            geometry::*,
            gradients::*,
            ui_node::*,
            ui_transform::*,
            widget::{ImageNode, InlineImage, Label, NodeImageMode, ViewportNode},
            UiScale,
        },
        // `eme_sprite` re-exports for texture slicing
        eme_sprite::{BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer},
        eme_text::{EmSize, RemSize, TextBackgroundColor},
    };
}

use eme_app::{prelude::*, AnimationSystems, HierarchyPropagatePlugin, PropagateSet};
use eme_camera::CameraUpdateSystems;
use eme_ecs::prelude::*;
use eme_input::InputSystems;
use layout::ui_surface::UiSurface;
use stack::ui_stack_system;
pub use stack::{ComputedStackIndex, UiStack};
use update::{propagate_ui_target_cameras, update_clipping_system};

/// The basic plugin for Eme UI
#[derive(Default)]
pub struct UiPlugin;

/// The label enum labeling the types of systems in the Eme UI
#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
pub enum UiSystems {
    /// After this label, input interactions with UI entities have been updated for this frame.
    ///
    /// Runs in [`PreUpdate`].
    Focus,
    /// All UI systems in [`PostUpdate`] will run in or after this label.
    Prepare,
    /// Propagate UI component values needed by layout.
    Propagate,
    /// Update content requirements before layout.
    Content,
    /// After this label, the UI layout state has been updated.
    ///
    /// Runs in [`PostUpdate`].
    Layout,
    /// UI systems ordered after [`UiSystems::Layout`].
    ///
    /// Runs in [`PostUpdate`].
    PostLayout,
    /// After this label, the [`UiStack`] resource has been updated.
    ///
    /// Runs in [`PostUpdate`].
    Stack,
}

/// The current scale of the UI.
///
/// A multiplier to fixed-sized UI values.
/// **Note:** This will only affect fixed UI values like [`Val::Px`]
#[derive(Debug, Reflect, Resource, Deref, DerefMut)]
#[reflect(Resource, Debug, Default)]
pub struct UiScale(pub f32);

impl Default for UiScale {
    fn default() -> Self {
        Self(1.0)
    }
}

// Marks systems that can be ambiguous with [`widget::text_system`] if the `eme_text` feature is enabled.
// See https://github.com/bevyengine/eme/pull/11391 for more details.
#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
struct AmbiguousWithText;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
struct AmbiguousWithUpdateText2dLayout;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiSurface>()
            .init_resource::<UiScale>()
            .init_resource::<UiStack>()
            .register_required_components::<
                eme_text::EditableText,
                widget::EditableTextContentSizeState,
            >()
            .configure_sets(
                PostUpdate,
                (
                    CameraUpdateSystems,
                    UiSystems::Prepare.after(AnimationSystems),
                    UiSystems::Propagate,
                    UiSystems::Content,
                    UiSystems::Layout,
                    UiSystems::PostLayout,
                )
                    .chain_weak(),
            )
            .configure_sets(
                PostUpdate,
                PropagateSet::<ComputedUiTargetCamera>::default().in_set(UiSystems::Propagate),
            )
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
                PostUpdate,
            ))
            .configure_sets(
                PostUpdate,
                PropagateSet::<ComputedUiRenderTargetInfo>::default().in_set(UiSystems::Propagate),
            )
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
                PostUpdate,
            ))
            .add_systems(
                PreUpdate,
                ui_focus_system.in_set(UiSystems::Focus).after(InputSystems),
            );

        #[cfg(feature = "eme_picking")]
        app.add_plugins(picking_backend::UiPickingPlugin)
            .add_systems(
                First,
                widget::viewport_picking.in_set(PickingSystems::PostInput),
            );

        app.add_systems(
            PostUpdate,
            (
                propagate_ui_target_cameras
                    .in_set(UiSystems::Prepare)
                    .before(eme_app::TransformGizmoRenderStep),
                ui_layout_system
                    .in_set(UiSystems::Layout)
                    .ambiguous_with(eme_sprite::update_text2d_layout),
                ui_stack_system.in_set(UiSystems::Stack),
                update_clipping_system.in_set(UiSystems::PostLayout),
                // Potential conflicts: `Assets<Image>`
                // They run independently since `widget::image_node_system` will only ever observe
                // its own ImageNode, and `widget::text_system` & `eme_text::update_text2d_layout`
                // will never modify a pre-existing `Image` asset.
                widget::update_image_content_size_system
                    .in_set(UiSystems::Content)
                    .in_set(AmbiguousWithText)
                    .in_set(AmbiguousWithUpdateText2dLayout),
                // Potential conflicts: `Assets<Image>`
                // `widget::text_system` and `eme_text::update_text2d_layout` run independently
                // since this system will only ever update viewport images.
                widget::update_viewport_render_target_size
                    .in_set(UiSystems::PostLayout)
                    .in_set(AmbiguousWithText)
                    .in_set(AmbiguousWithUpdateText2dLayout),
            ),
        );

        app.add_plugins(accessibility::AccessibilityPlugin);

        app.add_observer(interaction_states::on_add_disabled)
            .add_observer(interaction_states::on_remove_disabled)
            .add_observer(interaction_states::on_add_checkable)
            .add_observer(interaction_states::on_remove_checkable)
            .add_observer(interaction_states::on_add_checked)
            .add_observer(interaction_states::on_remove_checked)
            .add_observer(interaction_states::on_add_selectable)
            .add_observer(interaction_states::on_remove_selectable)
            .add_observer(interaction_states::on_add_selected)
            .add_observer(interaction_states::on_remove_selected);

        build_text_interop(app);
    }
}

fn build_text_interop(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (
            widget::update_inline_image_boxes
                .before(detect_text_needs_rerender)
                .in_set(UiSystems::Content),
            widget::measure_text_system
                .after(detect_text_needs_rerender)
                .after(eme_text::load_font_assets_into_font_collection)
                .in_set(UiSystems::Content)
                // Text and Text2d are independent.
                // Potential conflict: `Assets<Image>`
                // Since both systems will only ever insert new [`Image`] assets,
                // they will never observe each other's effects.
                .ambiguous_with(eme_sprite::update_text2d_layout)
                // We assume Text is on disjoint UI entities to ImageNode and UiTextureAtlasImage
                // FIXME: Add an archetype invariant for this https://github.com/bevyengine/eme/issues/1481.
                .ambiguous_with(widget::update_image_content_size_system)
                .ambiguous_with(EditableTextSystems),
            widget::text_system
                .in_set(UiSystems::PostLayout)
                .after(eme_text::load_font_assets_into_font_collection)
                .before(eme_asset::AssetEventSystems)
                // Text2d and eme_ui text are entirely on separate entities
                .ambiguous_with(eme_sprite::update_text2d_layout)
                .ambiguous_with(eme_sprite::calculate_bounds_text2d),
            (
                widget::update_editable_text_content_size,
                widget::update_editable_text_styles,
            )
                .chain()
                .in_set(UiSystems::Content)
                .after(eme_text::load_font_assets_into_font_collection)
                .before(EditableTextSystems)
                .ambiguous_with(widget::update_image_content_size_system)
                .ambiguous_with(widget::measure_text_system)
                .ambiguous_with(eme_sprite::update_text2d_layout),
            widget::sync_editable_text_viewports
                .after(UiSystems::Layout)
                .before(EditableTextSystems),
            widget::update_editable_text_layout
                .in_set(UiSystems::PostLayout)
                // This is unlikely to result in real conflicts,
                // as FocusChangeEvents only mutates internal state of InputFocus,
                // and editable_text_system only reads from it.
                // However, in case this changes in the future, this is a safer choice,
                // as editable_text_system or related systems could generate focus changes
                // which should be processed ASAP.
                .before(eme_input_focus::InputFocusSystems::FocusChangeEvents)
                .before(eme_asset::AssetEventSystems)
                .ambiguous_with(widget::text_system)
                .ambiguous_with(eme_sprite::update_text2d_layout)
                .ambiguous_with(eme_sprite::calculate_bounds_text2d),
            sync_font_size_to_em_size
                .in_set(UiSystems::Content)
                .after(eme_text::load_font_assets_into_font_collection),
        ),
    );

    app.configure_sets(
        PostUpdate,
        AmbiguousWithText.ambiguous_with(widget::text_system),
    );

    app.configure_sets(
        PostUpdate,
        AmbiguousWithUpdateText2dLayout.ambiguous_with(eme_sprite::update_text2d_layout),
    );

    // We cannot set this up in eme_text as this would create a circular dependency between eme_ui and eme_text
    app.configure_sets(
        PostUpdate,
        EditableTextSystems
            .after(UiSystems::Layout)
            .before(UiSystems::PostLayout),
    );
}
