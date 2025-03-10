use crate::method;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::visit::Visit;
use syn::visit_mut::VisitMut;
use syn::{parse2, FnArg, Ident, ImplItem, ItemImpl, Path};

#[derive(Default)]
struct TypeParameterVisitor(Vec<String>);

impl<'ast> Visit<'ast> for TypeParameterVisitor {
    fn visit_path_segment(&mut self, path_seg: &'ast syn::PathSegment) {
        self.visit_path_arguments(&path_seg.arguments);

        self.0.push(path_seg.ident.to_string());
    }
    fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
        self.0.push(lifetime.ident.to_string());
    }
}

struct QualifiesAssociatedTypes(Path, Vec<Ident>);
impl VisitMut for QualifiesAssociatedTypes {
    fn visit_type_path_mut(&mut self, type_path: &mut syn::TypePath) {
        type_path
            .path
            .segments
            .iter_mut()
            .for_each(|segment| self.visit_path_segment_mut(segment));
        if let Some(ref mut qself) = &mut type_path.qself {
            self.visit_qself_mut(qself);
        } else {
            let first_and_second: Vec<_> = type_path
                .path
                .segments
                .clone()
                .into_iter()
                .take(2)
                .collect();
            if let (Some(first), Some(second)) = (first_and_second.get(0), first_and_second.get(1))
            {
                let trait_ = &self.0;
                let trailing = type_path.path.segments.iter().skip(1);
                if first.ident.to_string() == "Self" && self.1.contains(&second.ident) {
                    *type_path = parse2(quote![<Self as #trait_>::#(#trailing)::*]).unwrap();
                }
            }
        }
    }
}

pub(crate) fn transform(mut input: ItemImpl) -> TokenStream {
    if let Some((_, path, _)) = input.trait_.clone() {
        let ty = path.clone();
        let associated_types: Vec<_> = input
            .items
            .iter()
            .filter_map(|item| {
                if let ImplItem::Type(associated_type) = item {
                    Some(associated_type.ident.clone())
                } else {
                    None
                }
            })
            .collect();
        QualifiesAssociatedTypes(ty, associated_types).visit_item_impl_mut(&mut input);
    }
    let generics = &input.generics;
    let mut type_params = TypeParameterVisitor::default();
    type_params.visit_type(&input.self_ty);
    let impl_generics: Vec<_> = input
        .generics
        .params
        .iter()
        .filter(|param| {
            let ident = match param {
                syn::GenericParam::Type(ty) => &ty.ident,
                syn::GenericParam::Lifetime(lifetime) => &lifetime.lifetime.ident,
                syn::GenericParam::Const(cons) => &cons.ident,
            };
            type_params.0.contains(&ident.to_string())
        })
        .collect();
    let struct_type = &input.self_ty;
    let mut trait_name = None;
    let trait_ = match &input.trait_ {
        Some((bang, path, for_)) => {
            trait_name = Some(path);
            quote! {
                #bang #path #for_
            }
        }
        None => TokenStream::default(),
    };

    let qualified_type = match trait_name {
        Some(trait_name) => quote![<#struct_type as #trait_name>],
        None => input.self_ty.to_token_stream(),
    };
    // Pretty print type name
    let type_name = qualified_type
        .to_string()
        .replace(" ,", ",")
        .replace(" >", ">")
        .replace(" <", "<")
        .replace("< ", "<");

    let (members, impl_members): (Vec<_>, Vec<_>) = input
        .items
        .iter()
        .map(|item| {
            if let ImplItem::Method(method) = item {
                if let Some(FnArg::Receiver(_)) = method.sig.inputs.first() {
                    method::transform(
                        quote![self.mry.mocks_write()],
                        quote![#qualified_type::],
                        &(type_name.clone() + "::"),
                        quote![self.mry.record_call_and_find_mock_output],
                        Some(&method.vis),
                        &method.attrs,
                        &method.sig,
                        &method.block.stmts.iter().fold(
                            TokenStream::default(),
                            |mut stream, item| {
                                item.to_tokens(&mut stream);
                                stream
                            },
                        ),
                    )
                } else {
                    method::transform(
                        quote![Box::new(mry::STATIC_MOCKS.write())],
                        quote![#qualified_type::],
                        &(type_name.clone() + "::"),
                        quote![mry::STATIC_MOCKS.write().record_call_and_find_mock_output],
                        Some(&method.vis),
                        &method.attrs,
                        &method.sig,
                        &method.block.stmts.iter().fold(
                            TokenStream::default(),
                            |mut stream, item| {
                                item.to_tokens(&mut stream);
                                stream
                            },
                        ),
                    )
                }
            } else {
                (item.to_token_stream(), TokenStream::default())
            }
        })
        .unzip();

    let impl_generics = if impl_generics.is_empty() {
        TokenStream::default()
    } else {
        quote!( <#(#impl_generics),*>)
    };

    quote! {
        impl #generics #trait_ #struct_type {
            #(#members)*
        }

        impl #impl_generics #struct_type {
            #(#impl_members)*
        }
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use syn::parse2;

    use super::*;

    #[test]
    fn keeps_attributes() {
        let input: ItemImpl = parse2(quote! {
            impl Cat {
                #[meow]
                #[meow]
                fn meow(#[a] &self, #[b] count: usize) -> String {
                    "meow".repeat(count)
                }
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                impl Cat {
                    #[meow]
                    #[meow]
                    fn meow(#[a] &self, #[b] count: usize) -> String {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&Cat::meow), "Cat::meow", (count.clone())) {
                            return out;
                        }
                        "meow".repeat(count)
                    }
                }

                impl Cat {
                    pub fn mock_meow<'mry>(&'mry mut self, arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&Cat::meow),
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
    fn support_generics() {
        let input: ItemImpl = parse2(quote! {
            impl<'a, A: Clone> Cat<'a, A> {
                fn meow<'a, B>(&'a self, count: usize) -> B {
                    "meow".repeat(count)
                }
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                impl<'a, A: Clone> Cat<'a, A> {
                    fn meow<'a, B>(&'a self, count: usize) -> B {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&Cat<'a, A>::meow), "Cat<'a, A>::meow", (count.clone())) {
                            return out;
                        }
                        "meow".repeat(count)
                    }
                }

                impl <'a, A: Clone> Cat<'a, A> {
                    pub fn mock_meow<'mry>(&'mry mut self, arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), B, mry::Behavior1<(usize), B> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&Cat<'a, A>::meow),
                            name: "Cat<'a, A>::meow",
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
    fn support_trait() {
        let input: ItemImpl = parse2(quote! {
            impl<A: Clone> Animal<A> for Cat {
                fn name(&self) -> String {
                    self.name
                }
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                impl<A: Clone> Animal<A> for Cat {
                    fn name(&self, ) -> String {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&<Cat as Animal<A> >::name), "<Cat as Animal<A>>::name", ()) {
                            return out;
                        }
                        self.name
                    }
                }

                impl Cat {
                    pub fn mock_name<'mry>(&'mry mut self,) -> mry::MockLocator<'mry, (), String, mry::Behavior0<(), String> > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&< Cat as Animal < A > >::name),
                            name: "<Cat as Animal<A>>::name",
                            matcher: Some(().into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn support_trait_with_associated_type() {
        let input: ItemImpl = parse2(quote! {
            impl Iterator for Cat {
                type Item = String;
                fn next(&self) -> Option<Self::Item> {
                    Some(self.name)
                }
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                impl Iterator for Cat {
                    type Item = String;
                    fn next(&self, ) -> Option< <Self as Iterator>::Item> {
                        if let Some(out) = self.mry.record_call_and_find_mock_output(std::any::Any::type_id(&<Cat as Iterator>::next), "<Cat as Iterator>::next", ()) {
                            return out;
                        }
                        Some(self.name)
                    }
                }

                impl Cat {
                    pub fn mock_next<'mry>(&'mry mut self,) -> mry::MockLocator<'mry, (), Option< <Self as Iterator>::Item >, mry::Behavior0<(), Option< <Self as Iterator>::Item> > > {
                        mry::MockLocator {
                            mocks: self.mry.mocks_write(),
                            key: std::any::Any::type_id(&<Cat as Iterator>::next),
                            name: "<Cat as Iterator>::next",
                            matcher: Some(().into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn support_associated_functions() {
        let input: ItemImpl = parse2(quote! {
            impl Cat {
                fn meow(count: usize) -> String {
                    "meow".repeat(count)
                }
            }
        })
        .unwrap();

        assert_eq!(
            transform(input).to_string(),
            quote! {
                impl Cat {
                    fn meow(count: usize) -> String {
                        if let Some(out) = mry::STATIC_MOCKS.write().record_call_and_find_mock_output(std::any::Any::type_id(&Cat::meow), "Cat::meow", (count.clone())) {
                            return out;
                        }
                        "meow".repeat(count)
                    }
                }

                impl Cat {
                    pub fn mock_meow<'mry>(arg0: impl Into<mry::Matcher<usize>>) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                        mry::MockLocator {
                            mocks: Box::new(mry::STATIC_MOCKS.write()),
                            key: std::any::Any::type_id(&Cat::meow),
                            name: "Cat::meow",
                            matcher: Some((arg0.into(), ).into()),
                            _phantom: Default::default(),
                        }
                    }
                }
            }
            .to_string()
        );
    }
}
