use eme_macro_utils::EmeManifest;
use syn::Path;

/// Returns the correct path for `eme_reflect`.
pub(crate) fn get_eme_reflect_path() -> Path {
    EmeManifest::shared(|manifest| manifest.get_path("eme_reflect"))
}
