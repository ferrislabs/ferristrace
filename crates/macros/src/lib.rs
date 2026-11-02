use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, Generics, Ident, ItemFn, ItemStruct, Token, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn repository(args: TokenStream, input: TokenStream) -> TokenStream {
    let registration = parse_macro_input!(args as Registration);
    let item = parse_macro_input!(input as ItemStruct);

    let name = &item.ident;
    let (_, type_generics, _) = item.generics.split_for_impl();
    let domain = &registration.domain;
    let backend = &registration.backend;
    let (impl_generics, _, where_clause) = registration.generics.split_for_impl();

    quote! {
        #item

        impl #impl_generics ::ferristrace_application::registry::RepoFor<
            ::ferristrace_application::registry::domain::#domain
        > for #backend #where_clause {
            type Repo = #name #type_generics;

            fn build(
                &self,
                tx: &<Self as ::ferristrace_application::registry::UnitOfWork>::Tx,
            ) -> Self::Repo {
                #name::new(self, tx)
            }
        }
    }
    .into()
}

#[proc_macro_attribute]
pub fn transactional(args: TokenStream, input: TokenStream) -> TokenStream {
    let listed = parse_macro_input!(args as Listed);
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = parse_macro_input!(input as ItemFn);

    let bindings = listed.domains.iter().map(|name| {
        let marker = Ident::new(&pascal_case(&name.to_string()), name.span());
        quote! {
            let #name = <
                <S as ::ferristrace_application::Stack>::Repos
                as ::ferristrace_application::registry::RepoFor<
                    ::ferristrace_application::registry::domain::#marker
                >
            >::build(&self.repos, &tx);
        }
    });
    let unavailable = &listed.unavailable;
    let settle = if listed.always {
        quote! { ::ferristrace_application::registry::Settle::Always }
    } else {
        quote! { ::ferristrace_application::registry::Settle::OnSuccess }
    };
    let statements = &block.stmts;

    quote! {
        #(#attrs)*
        #vis #sig {
            ::ferristrace_application::registry::UnitOfWork::transaction(
                &self.repos,
                || #unavailable,
                #settle,
                move |tx| async move {
                    #(#bindings)*
                    #(#statements)*
                },
            )
            .await
        }
    }
    .into()
}

struct Registration {
    domain: Ident,
    backend: Type,
    generics: Generics,
}

impl Parse for Registration {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut domain = None;
        let mut backend = None;
        let mut generics = Generics::default();

        while !input.is_empty() {
            let key = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "domain" => domain = Some(input.parse::<Ident>()?),
                "backend" => backend = Some(input.parse::<Type>()?),
                "generics" => {
                    generics = input.parse::<Generics>()?;
                    generics.where_clause = input.parse()?;
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown key `{other}`, expected `domain`, `backend` or `generics`"
                        ),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            domain: domain.ok_or_else(|| missing("domain"))?,
            backend: backend.ok_or_else(|| missing("backend"))?,
            generics,
        })
    }
}

struct Listed {
    domains: Vec<Ident>,
    unavailable: Expr,
    always: bool,
}

impl Parse for Listed {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut domains = Vec::new();
        let mut unavailable = None;
        let mut always = false;

        for entry in Punctuated::<Entry, Token![,]>::parse_terminated(input)? {
            match entry {
                Entry::Domain(name) => domains.push(name),
                Entry::Unavailable(error) => unavailable = Some(error),
                Entry::Settle(policy) if policy == "always" => always = true,
                Entry::Settle(policy) if policy == "on_success" => always = false,
                Entry::Settle(policy) => {
                    return Err(syn::Error::new(
                        policy.span(),
                        "expected `settle = always` or `settle = on_success`",
                    ));
                }
            }
        }

        Ok(Self {
            domains,
            unavailable: unavailable.ok_or_else(|| missing("unavailable"))?,
            always,
        })
    }
}

enum Entry {
    Domain(Ident),
    Unavailable(Expr),
    Settle(Ident),
}

impl Parse for Entry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse::<Ident>()?;
        if name == "unavailable" && input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            return Ok(Self::Unavailable(input.parse()?));
        }
        if name == "settle" && input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            return Ok(Self::Settle(input.parse()?));
        }
        Ok(Self::Domain(name))
    }
}

fn missing(key: &str) -> syn::Error {
    syn::Error::new(Span::call_site(), format!("missing required key `{key}`"))
}

fn pascal_case(snake: &str) -> String {
    snake
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut letters = part.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        })
        .collect()
}
