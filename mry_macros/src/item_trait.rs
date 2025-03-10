use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::visit::Visit;
use syn::{Ident, ItemTrait};

use crate::method;

#[derive(Default)]
struct AsyncTraitFindVisitor(bool);

impl<'ast> Visit<'ast> for AsyncTraitFindVisitor {
    fn visit_trait_item_method(&mut self, i: &'ast syn::TraitItemMethod) {
        if i.sig.asyncness.is_some() {
            self.0 = true;
        }
    }
}

pub(crate) fn transform(input: ItemTrait) -> TokenStream {
    let mut async_trait_finder = AsyncTraitFindVisitor::default();
    async_trait_finder.visit_item_trait(&input);
    let async_trait_or_blank = if async_trait_finder.0 {
        quote!(#[async_trait::async_trait])
    } else {
        TokenStream::default()
    };

    let generics = &input.generics;
    let trait_ident = &input.ident;
    let mry_ident = Ident::new(&format!("Mock{}", &input.ident), Span::call_site());
    let vis = &input.vis;
    let panic_message = format!("mock not found for {}", trait_ident);
    let (items, impl_items): (Vec<_>, Vec<_>) = input
        .items
        .iter()
        .map(|item| match item {
            syn::TraitItem::Method(method) => method::transform(
                quote![self.mry.mocks_write()],
                quote![#mry_ident::],
                &(trait_ident.to_string() + "::"),
                quote![self.mry.record_call_and_find_mock_output],
                None,
                &method.attrs,
                &method.sig,
                &method
                    .default
                    .as_ref()
                    .map(|default| default.to_token_stream())
                    .unwrap_or(quote![panic!(#panic_message)]),
            ),
            _item => todo!(),
        })
        .unzip();

    quote! {
        #input

        #[derive(Default, Clone)]
        #vis struct #mry_ident {
            pub mry: mry::Mry,
        }

        #async_trait_or_blank
        impl #generics #trait_ident for #mry_ident {
            #(#items)*
        }

        impl #mry_ident {
            #(#impl_items)*
        }
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use syn::parse2;

    use super::*;

    #[test]
    fn add_mry_object() {
        let input: ItemTrait = parse2(quote! {
            trait Cat {
                fn meow(&self, count: usize) -> String;
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
				trait Cat {
					fn meow(&self, count: usize) -> String;
				}

				#[derive(Default, Clone)]
				struct MockCat {
					pub mry : mry::Mry,
				}

                impl Cat for MockCat {
                    fn meow(&self, count: usize) -> String {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&MockCat::meow), "Cat::meow", (count.clone())) {
                            return out;
                        }
                        panic!("mock not found for Cat")
                    }
                }

                impl MockCat {
                    pub fn mock_meow<'mry>(&'mry mut self, arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&MockCat::meow),
                            name: "Cat::meow",
                            matcher: Some((arg0.into(),).into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn respects_attributes_and_visibility() {
        let input: ItemTrait = parse2(quote! {
            pub trait Cat {
                fn meow(&self, count: usize) -> String;
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
				pub trait Cat {
					fn meow(&self, count: usize) -> String;
				}

				#[derive(Default, Clone)]
				pub struct MockCat {
					pub mry : mry::Mry,
				}

                impl Cat for MockCat {
                    fn meow(&self, count: usize) -> String {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&MockCat::meow), "Cat::meow", (count.clone())) {
                            return out;
                        }
                        panic!("mock not found for Cat")
                    }
                }

                impl MockCat {
                    pub fn mock_meow<'mry>(&'mry mut self, arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&MockCat::meow),
                            name: "Cat::meow",
                            matcher: Some((arg0.into(),).into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn supports_async_trait() {
        let input: ItemTrait = parse2(quote! {
            #[async_trait::async_trait]
            trait Cat {
                async fn meow(&self, count: usize) -> String;
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                #[async_trait::async_trait]
				trait Cat {
					async fn meow(&self, count: usize) -> String;
				}

				#[derive(Default, Clone)]
				struct MockCat {
					pub mry : mry::Mry,
				}

                #[async_trait::async_trait]
                impl Cat for MockCat {
                    async fn meow(&self, count: usize) -> String {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&MockCat::meow), "Cat::meow", (count.clone())) {
                            return out;
                        }
                        panic!("mock not found for Cat")
                    }
                }

                impl MockCat {
                    pub fn mock_meow<'mry>(&'mry mut self, arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&MockCat::meow),
                            name: "Cat::meow",
                            matcher: Some((arg0.into(),).into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }
}
