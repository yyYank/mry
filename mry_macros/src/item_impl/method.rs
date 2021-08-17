use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{FnArg, Ident, ImplItemMethod, Pat, PatIdent, ReturnType, Type};

pub fn transform(struct_name: &str, method: &ImplItemMethod) -> (TokenStream, TokenStream) {
    // Split into receiver and other inputs
    let receiver;
    let mut inputs = method.sig.inputs.iter();
    if let Some(FnArg::Receiver(rcv)) = inputs.next() {
        receiver = rcv;
    } else {
        return (method.to_token_stream(), TokenStream::default());
    }
    let inputs: Vec<_> = inputs
        .map(|input| {
            if let FnArg::Typed(typed_arg) = input {
                typed_arg.clone()
            } else {
                panic!("multiple receiver?");
            }
        })
        .collect();
    let mut bindings = Vec::new();

    let generics = &method.sig.generics;
    let body = &method.block;
    let attrs = method.attrs.clone();
    let ident = method.sig.ident.clone();
    let mock_ident = Ident::new(&format!("mock_{}", ident), Span::call_site());
    let name = format!("{}::{}", struct_name, ident.to_string());
    let args_with_type: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            if let Pat::Ident(_) = *input.pat {
                input.clone()
            } else {
                let pat = input.pat.clone();
                let arg_name = Ident::new(&format!("arg{}", i), Span::call_site());
                bindings.push((pat, arg_name.clone()));
                let ident = Pat::Ident(PatIdent {
                    attrs: Default::default(),
                    by_ref: Default::default(),
                    mutability: Default::default(),
                    ident: arg_name,
                    subpat: Default::default(),
                });
                let mut arg_with_type = (*input).clone();
                arg_with_type.pat = Box::new(ident.clone());
                arg_with_type
            }
        })
        .collect();
    let derefed_input_type_tuple: Vec<_> = args_with_type
        .iter()
        .map(|input| {
            if is_str(&input.ty) {
                return quote!(String);
            }
            let ty = match &*input.ty {
                Type::Reference(ty) => {
                    let ty = &ty.elem;
                    quote!(#ty)
                }
                ty => quote!(#ty),
            };
            ty
        })
        .collect();
    let derefed_input: Vec<_> = args_with_type
        .iter()
        .map(|input| {
            let pat = &input.pat;
            if is_str(&input.ty) {
                return quote!(#pat.to_string());
            }
            let input = match &*input.ty {
                Type::Reference(_ty) => {
                    quote!(*#pat)
                }
                _ => quote!(#pat),
            };
            input
        })
        .collect();
    let output_type = match &method.sig.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ty) => quote!(#ty),
    };
    let asyn = &method.sig.asyncness;
    let vis = &method.vis;
    let args_with_type = quote!(#(#args_with_type),*);
    let input_type_tuple = quote!((#(#derefed_input_type_tuple),*));
    let derefed_input_tuple = quote!((#(#derefed_input),*));
    let bindings = bindings.iter().map(|(pat, arg)| quote![let #pat = #arg;]);
    let behavior_name = Ident::new(&format!("Behavior{}", inputs.len()), Span::call_site());
    let behavior_type = quote! {
        mry::#behavior_name<#input_type_tuple, #output_type>
    };
    (
        quote! {
            #(#attrs)*
            #vis #asyn fn #ident #generics(#receiver, #args_with_type) -> #output_type {
                #[cfg(test)]
                if self.mry.is_some() {
                    return mry::MOCK_DATA
                        .lock()
                        .get_mut_or_create::<#input_type_tuple, #output_type>(&self.mry, #name)
                        ._inner_called(&#derefed_input_tuple);
                }
                #(#bindings)*
                #body
            }
        },
        quote! {
            #[cfg(test)]
            pub fn #mock_ident<'mry>(&'mry mut self) -> mry::MockLocator<'mry, #input_type_tuple, #output_type, #behavior_type> {
                if self.mry.is_none() {
                    self.mry = mry::Mry::generate();
                }
                mry::MockLocator {
                    id: &self.mry,
                    name: #name,
                    _phantom: Default::default(),
                }
            }
        },
    )
}

fn is_str(ty: &Type) -> bool {
    match ty {
        Type::Reference(ty) => {
            if let Type::Path(path) = &*ty.elem {
                if let Some(ident) = path.path.get_ident() {
                    return ident.to_string() == "str";
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use similar_asserts::assert_eq;
    use syn::{parse2, ImplItemMethod};

    trait ToString {
        fn to_string(&self) -> String;
    }

    impl ToString for (TokenStream, TokenStream) {
        fn to_string(&self) -> String {
            (self.0.to_string() + " " + &self.1.to_string())
                .to_string()
                .trim()
                .to_string()
        }
    }

    #[test]
    fn support_associated_functions() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow() -> String{
                "meow"
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                fn meow() -> String {
                    "meow"
                }
            }
            .to_string()
        );
    }

    #[test]
    fn adds_mock_function() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow(&self, count: usize) -> String {
                "meow".repeat(count)
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                fn meow(&self, count: usize) -> String {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(usize), String>(&self.mry, "Cat::meow")
                            ._inner_called(&(count));
                    }
                    {
                        "meow".repeat(count)
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn empty_args() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow(&self) -> String {
                "meow".into()
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                fn meow(&self, ) -> String {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(), String>(&self.mry, "Cat::meow")
                            ._inner_called(&());
                    }
                    {
                        "meow".into()
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (), String, mry::Behavior0<(), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn multiple_args() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow(&self, base: String, count: usize) -> String {
                base.repeat(count)
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                fn meow(&self, base: String, count: usize) -> String {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(String, usize), String>(&self.mry, "Cat::meow")
                            ._inner_called(&(base, count));
                    }
                    {
                        base.repeat(count)
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (String, usize), String, mry::Behavior2<(String, usize), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn input_reference_and_str() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow(&self, out: &'static mut String, base: &str, count: &usize) {
                *out = base.repeat(count);
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                fn meow(&self, out: &'static mut String, base: &str, count: &usize) -> () {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(String, String, usize), ()>(&self.mry, "Cat::meow")
                            ._inner_called(&(*out, base.to_string(), *count));
                    }
                    {
                        *out = base.repeat(count);
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (String, String, usize), (), mry::Behavior3<(String, String, usize), ()> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn supports_async() {
        let input: ImplItemMethod = parse2(quote! {
            async fn meow(&self, count: usize) -> String{
                base().await.repeat(count);
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                async fn meow(&self, count: usize) -> String{
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(usize), String>(&self.mry, "Cat::meow")
                            ._inner_called(&(count));
                    }
                    {
                        base().await.repeat(count);
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn support_pattern() {
        let input: ImplItemMethod = parse2(quote! {
            fn meow(&self, A { name }: A, count: usize, _: String) -> String {
                name.repeat(count)
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
				fn meow(&self, arg0: A, count: usize, arg2: String) -> String {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(A, usize, String), String>(&self.mry, "Cat::meow")
                            ._inner_called(&(arg0, count, arg2));
                    }
					let A { name } = arg0;
					let _ = arg2;
                    {
						name.repeat(count)
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (A, usize, String), String, mry::Behavior3<(A, usize, String), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn respect_visibility() {
        let input: ImplItemMethod = parse2(quote! {
            pub fn meow(&self, count: usize) -> String {
                "meow".repeat(count)
            }
        })
        .unwrap();

        assert_eq!(
            transform("Cat", &input).to_string(),
            quote! {
                pub fn meow(&self, count: usize) -> String {
                    #[cfg(test)]
                    if self.mry.is_some() {
                        return mry::MOCK_DATA
                            .lock()
                            .get_mut_or_create::<(usize), String>(&self.mry, "Cat::meow")
                            ._inner_called(&(count));
                    }
                    {
                        "meow".repeat(count)
                    }
                }

                #[cfg(test)]
                pub fn mock_meow<'mry>(&'mry mut self) -> mry::MockLocator<'mry, (usize), String, mry::Behavior1<(usize), String> > {
                    if self.mry.is_none() {
                        self.mry = mry::Mry::generate();
                    }
                    mry::MockLocator {
                        id: &self.mry,
                        name: "Cat::meow",
                        _phantom: Default::default(),
                    }
                }
            }
            .to_string()
        );
    }
}
