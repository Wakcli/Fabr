use eme_asset::Handle;
use eme_derive::{Deref, DerefMut};
use eme_ecs::{component::Component, prelude::ReflectComponent, template::FromTemplate};
use eme_mesh::Mesh;
use eme_pbr::{MeshMaterial3d, StandardMaterial};
use eme_reflect::{prelude::ReflectDefault, Reflect};
use eme_render::sync_world::SyncToRenderWorld;
use eme_transform::components::Transform;
use derive_more::derive::From;

/// A mesh component used for raytracing.
///
/// The mesh used in this component must have [`Mesh::enable_raytracing`] set to true,
/// use the following set of vertex attributes: `{POSITION, NORMAL, UV_0, TANGENT}`, use [`eme_mesh::PrimitiveTopology::TriangleList`],
/// and use [`eme_mesh::Indices::U32`].
///
/// The material used for this entity must be [`MeshMaterial3d<StandardMaterial>`].
#[derive(
    Component, FromTemplate, Clone, Debug, Default, Deref, DerefMut, Reflect, PartialEq, Eq, From,
)]
#[reflect(Component, Default, Clone, PartialEq)]
#[require(MeshMaterial3d<StandardMaterial>, Transform, SyncToRenderWorld)]
pub struct RaytracingMesh3d(pub Handle<Mesh>);
