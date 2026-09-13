use crate::{
    impls::{common_partial_reflect_methods, impl_full_reflect, impl_type_path, impl_typed},
    struct_utility::FieldAccessors,
    ReflectStruct,
};
use eme_macro_utils::fq_std::{FQDefault, FQIterator, FQOption, FQResult};
use quote::{quote, ToTokens};

/// Implements `TupleStruct`, `GetTypeRegistration`, and `Reflect` for the given derive data.
pub(crate) fn impl_tuple_struct(reflect_struct: &ReflectStruct) -> proc_macro2::TokenStream {
    let fqoption = FQOption.into_token_stream();

    let eme_reflect_path = reflect_struct.meta().eme_reflect_path();
    let struct_path = reflect_struct.meta().type_path();

    let FieldAccessors {
        fields_ref,
        fields_mut,
        field_indices,
        field_count,
        ..
    } = FieldAccessors::new(reflect_struct);

    let where_clause_options = reflect_struct.where_clause_options();
    let get_type_registration_impl = reflect_struct.get_type_registration(&where_clause_options);

    let typed_impl = impl_typed(&where_clause_options, reflect_struct.to_info_tokens(true));

    let type_path_impl = impl_type_path(reflect_struct.meta());
    let full_reflect_impl = impl_full_reflect(&where_clause_options);
    let common_methods = common_partial_reflect_methods(
        reflect_struct.meta(),
        || Some(quote!(#eme_reflect_path::tuple_struct::tuple_struct_partial_eq)),
        || None,
        || Some(quote!(#eme_reflect_path::tuple_struct::tuple_struct_partial_cmp)),
    );
    let clone_fn = reflect_struct.get_clone_impl();

    #[cfg(not(feature = "functions"))]
    let function_impls = None::<proc_macro2::TokenStream>;
    #[cfg(feature = "functions")]
    let function_impls = crate::impls::impl_function_traits(&where_clause_options);

    let (impl_generics, ty_generics, where_clause) = reflect_struct
        .meta()
        .type_path()
        .generics()
        .split_for_impl();

    #[cfg(not(feature = "auto_register"))]
    let auto_register = None::<proc_macro2::TokenStream>;
    #[cfg(feature = "auto_register")]
    let auto_register = crate::impls::reflect_auto_registration(reflect_struct.meta());

    let where_reflect_clause = where_clause_options.extend_where_clause(where_clause);

    quote! {
        #get_type_registration_impl

        #typed_impl

        #type_path_impl

        #full_reflect_impl

        #function_impls

        #auto_register

        impl #impl_generics #eme_reflect_path::tuple_struct::TupleStruct for #struct_path #ty_generics #where_reflect_clause {
            fn field(&self, index: usize) -> #FQOption<&dyn #eme_reflect_path::PartialReflect> {
                match index {
                    #(#field_indices => #fqoption::Some(#fields_ref),)*
                    _ => #FQOption::None,
                }
            }

            fn field_mut(&mut self, index: usize) -> #FQOption<&mut dyn #eme_reflect_path::PartialReflect> {
                match index {
                    #(#field_indices => #fqoption::Some(#fields_mut),)*
                    _ => #FQOption::None,
                }
            }
            #[inline]
            fn field_len(&self) -> usize {
                #field_count
            }
            #[inline]
            fn iter_fields(&self) -> #eme_reflect_path::tuple_struct::TupleStructFieldIter {
                #eme_reflect_path::tuple_struct::TupleStructFieldIter::new(self)
            }

            fn to_dynamic_tuple_struct(&self) -> #FQResult<#eme_reflect_path::tuple_struct::DynamicTupleStruct, #eme_reflect_path::ReflectCloneError> {
                let mut dynamic: #eme_reflect_path::tuple_struct::DynamicTupleStruct = #FQDefault::default();
                dynamic.set_represented_type(#eme_reflect_path::PartialReflect::get_represented_type_info(self));
                #(dynamic.insert_boxed(#eme_reflect_path::PartialReflect::to_dynamic(#fields_ref)?);)*
                #FQResult::Ok(dynamic)
            }
        }

        impl #impl_generics #eme_reflect_path::PartialReflect for #struct_path #ty_generics #where_reflect_clause {
            #[inline]
            fn get_represented_type_info(&self) -> #FQOption<&'static #eme_reflect_path::TypeInfo> {
                #FQOption::Some(<Self as #eme_reflect_path::Typed>::type_info())
            }

            #[inline]
            fn try_apply(
                &mut self,
                value: &dyn #eme_reflect_path::PartialReflect
            ) -> #FQResult<(), #eme_reflect_path::ApplyError> {
                if let #eme_reflect_path::ReflectRef::TupleStruct(struct_value) =
                    #eme_reflect_path::PartialReflect::reflect_ref(value) {
                    for (i, value) in #FQIterator::enumerate(#eme_reflect_path::tuple_struct::TupleStruct::iter_fields(struct_value)) {
                        if let #FQOption::Some(v) = #eme_reflect_path::tuple_struct::TupleStruct::field_mut(self, i) {
                            #eme_reflect_path::PartialReflect::try_apply(v, value)?;
                        }
                    }
                } else {
                    return #FQResult::Err(
                        #eme_reflect_path::ApplyError::MismatchedKinds {
                            from_kind: #eme_reflect_path::PartialReflect::reflect_kind(value),
                            to_kind: #eme_reflect_path::ReflectKind::TupleStruct,
                        }
                    );
                }
               #FQResult::Ok(())
            }
            #[inline]
            fn reflect_kind(&self) -> #eme_reflect_path::ReflectKind {
                #eme_reflect_path::ReflectKind::TupleStruct
            }
            #[inline]
            fn reflect_ref(&self) -> #eme_reflect_path::ReflectRef {
                #eme_reflect_path::ReflectRef::TupleStruct(self)
            }
            #[inline]
            fn reflect_mut(&mut self) -> #eme_reflect_path::ReflectMut {
                #eme_reflect_path::ReflectMut::TupleStruct(self)
            }
            #[inline]
            fn reflect_owned(self: #eme_reflect_path::__macro_exports::alloc_utils::Box<Self>) -> #eme_reflect_path::ReflectOwned {
                #eme_reflect_path::ReflectOwned::TupleStruct(self)
            }

            #common_methods

            #clone_fn
        }
    }
}
