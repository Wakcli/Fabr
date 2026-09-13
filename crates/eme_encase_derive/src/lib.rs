#![expect(missing_docs, reason = "Not all docs are written yet, see #3492.")]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://eme.org/assets/icon.png",
    html_favicon_url = "https://eme.org/assets/icon.png"
)]

use eme_macro_utils::EmeManifest;
use encase_derive_impl::{implement, syn};

const ENCASE: &str = "encase";

fn eme_encase_path() -> syn::Path {
    EmeManifest::shared(|eme_manifest| {
        eme_manifest
            .maybe_get_path("eme_render")
            .map(|eme_render_path| {
                let mut segments = eme_render_path.segments;
                segments.push(EmeManifest::parse_str("render_resource"));
                syn::Path {
                    leading_colon: None,
                    segments,
                }
            })
            .map(|path| {
                let mut segments = path.segments;
                segments.push(EmeManifest::parse_str(ENCASE));
                syn::Path {
                    leading_colon: None,
                    segments,
                }
            })
            .unwrap_or_else(|| eme_manifest.get_path(ENCASE))
    })
}

implement!(eme_encase_path());
