use eme_reflect_derive::{impl_reflect_opaque, impl_type_path};

impl_type_path!(::eme_platform::hash::NoOpHash);
impl_type_path!(::eme_platform::hash::FixedHasher);
impl_type_path!(::eme_platform::hash::PassHash);

impl_reflect_opaque!(::eme_platform::hash::Hashed<T: Clone + Send + Sync, H: Send + Sync>());
