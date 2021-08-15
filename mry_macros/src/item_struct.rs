use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, ItemStruct};

pub(crate) fn transform(input: ItemStruct) -> TokenStream {
    let struct_name = input.ident;
    let attrs = input.attrs;
    let struct_fields = input
        .fields
        .iter()
        .map(|field| {
            let attrs = field.attrs.clone();
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote! {
                #(#attrs)*
                #name: #ty
            }
        })
        .collect::<Vec<_>>();
    let struct_field_names = input
        .fields
        .iter()
        .map(|field| &field.ident)
        .collect::<Vec<_>>();
    let mry_struct_name = Ident::new(&format!("Mry{}", struct_name), Span::call_site());

    quote! {
        #(#attrs)*
        struct #struct_name {
            #(#struct_fields),*,
            #[cfg(test)]
            mry_id: mry::MryId,
        }

        #(#attrs)*
        struct #mry_struct_name {
            #(#struct_fields),*,
        }

        impl From<#mry_struct_name> for #struct_name {
            fn from(#mry_struct_name {#(#struct_field_names),*}: #mry_struct_name) -> Self {
                #struct_name {
                    #(#struct_field_names),*,
                    #[cfg(test)]
                    mry_id: Default::default(),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use similar_asserts::assert_eq;
    use syn::parse2;

    use super::*;

    #[test]
    fn adds_mry_id() {
        let input: ItemStruct = parse2(quote! {
            struct Cat {
                name: String,
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                struct Cat {
                    name: String,
                    #[cfg(test)]
                    mry_id : mry::MryId,
                }

                struct MryCat {
                    name: String,
                }

                impl From<MryCat> for Cat {
                    fn from (MryCat { name }: MryCat) -> Self {
                        Cat {
                            name,
                            #[cfg(test)] mry_id: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn keep_attributes() {
        let input: ItemStruct = parse2(quote! {
            #[derive(Clone, Default)]
            struct Cat {
                #[name]
                name: String,
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                #[derive(Clone, Default)]
                struct Cat {
                    #[name]
                    name: String,
                    #[cfg(test)]
                    mry_id : mry::MryId,
                }

                #[derive(Clone, Default)]
                struct MryCat {
                    #[name]
                    name: String,
                }

                impl From<MryCat> for Cat {
                    fn from (MryCat { name }: MryCat) -> Self {
                        Cat {
                            name,
                            #[cfg(test)] mry_id: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }
}