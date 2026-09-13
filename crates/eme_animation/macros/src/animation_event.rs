use eme_macro_utils::EmeManifest;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn derive_animation_event(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let (eme_ecs, eme_animation) = EmeManifest::shared(|manifest| {
        let eme_ecs = manifest.get_path("eme_ecs");
        let eme_animation = manifest.get_path("eme_animation");
        (eme_ecs, eme_animation)
    });

    let generics = ast.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let struct_name = &ast.ident;

    quote! {
        impl #impl_generics #eme_ecs::event::Event for #struct_name #type_generics #where_clause {
            type Trigger<'a> = #eme_animation::AnimationEventTrigger;
        }

        impl #impl_generics #eme_animation::AnimationEvent for #struct_name #type_generics #where_clause {
        }
    }
    .into()
}
