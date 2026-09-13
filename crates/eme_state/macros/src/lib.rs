#![cfg_attr(docsrs, feature(doc_cfg))]

//! Macros for deriving `States` and `SubStates` traits.

extern crate proc_macro;

mod states;

use eme_macro_utils::EmeManifest;
use proc_macro::TokenStream;

/// Implements the `States` trait for a type - see the trait
/// docs for an example usage.
#[proc_macro_derive(States, attributes(states))]
pub fn derive_states(input: TokenStream) -> TokenStream {
    states::derive_states(input)
}

/// Implements the `SubStates` trait for a type - see the trait
/// docs for an example usage.
#[proc_macro_derive(SubStates, attributes(states, source))]
pub fn derive_substates(input: TokenStream) -> TokenStream {
    states::derive_substates(input)
}

pub(crate) fn eme_state_path() -> syn::Path {
    EmeManifest::shared(|manifest| manifest.get_path("eme_state"))
}
