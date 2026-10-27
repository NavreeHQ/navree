//! `#[scopes]`: local scopes as query builder methods.
//!
//! The `#[scope]` functions of the impl block stay where they are (minus
//! the attribute). A trait, `{Model}Scopes` by default, gets one method per
//! scope that applies it to the builder (`Scoped::apply_scope`, which keeps
//! the scope's conditions grouped), and is implemented for every builder of
//! the model.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::meta::ParseNestedMeta;
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{FnArg, Ident, ImplItem, ItemImpl, LitStr, Type, Visibility};

use crate::errors::{Errors, key_name};
use crate::paths;

/// `#[scopes(name = "...", vis = "...")]`.
#[derive(Default)]
struct Args {
    name: Option<Ident>,
    vis: Option<Visibility>,
}

fn parse_arg(args: &mut Args, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.path.is_ident("name") {
        let name: LitStr = meta.value()?.parse()?;
        args.name = Some(name.parse()?);
    } else if meta.path.is_ident("vis") {
        let vis: LitStr = meta.value()?.parse()?;
        args.vis = Some(if vis.value().trim().is_empty() {
            Visibility::Inherited
        } else {
            vis.parse()?
        });
    } else {
        return Err(meta.error(format!(
            "unknown scopes option `{}`; expected `name` (the trait's name) or `vis` (its visibility)",
            key_name(&meta.path)
        )));
    }
    Ok(())
}

/// Replaces `Self` in a scope's parameter types with the model type (in
/// the generated trait, `Self` is the builder).
struct ReplaceSelf<'a> {
    model: &'a Type,
    krate: &'a TokenStream,
}

impl VisitMut for ReplaceSelf<'_> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(path) = ty
            && path.qself.is_none()
            && path.path.segments.first().is_some_and(|segment| segment.ident == "Self")
        {
            let model = self.model;
            let krate = self.krate;
            let rest: Vec<_> = path.path.segments.iter().skip(1).collect();
            *ty = if rest.is_empty() {
                model.clone()
            } else {
                // `Self::Key` names the model's key type.
                syn::parse_quote!(<#model as #krate::Model>::#(#rest)::*)
            };
            return;
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}

/// The impl block without its `#[scope]` markers (what stays of it when
/// `#[scopes]` reports an error); nothing when it is not an impl block.
pub(crate) fn without_markers(item: TokenStream) -> TokenStream {
    let Ok(mut block) = syn::parse2::<ItemImpl>(item) else {
        return TokenStream::new();
    };
    for item in &mut block.items {
        if let ImplItem::Fn(function) = item {
            function.attrs.retain(|attr| !attr.path().is_ident("scope"));
        }
    }
    quote!(#block)
}

/// Expands `#[scopes]` on an impl block.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let mut options = Args::default();
    let parser = syn::meta::parser(|meta| parse_arg(&mut options, &meta));
    syn::parse::Parser::parse2(parser, args)?;

    let mut block: ItemImpl = syn::parse2(item)?;
    if let Some((path, _)) = &block.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "#[scopes] goes on the model's own impl block (`impl Deal { ... }`), not on a trait impl",
        ));
    }
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new(
            block.generics.span(),
            "#[scopes] does not take an impl block with generic parameters",
        ));
    }
    let model = (*block.self_ty).clone();
    let Type::Path(model_path) = &model else {
        return Err(syn::Error::new(model.span(), "#[scopes] needs a model type"));
    };
    let Some(model_ident) = model_path.path.segments.last().map(|segment| segment.ident.clone()) else {
        return Err(syn::Error::new(model.span(), "#[scopes] needs a model type"));
    };
    let krate = paths::orm(model_ident.span())?;
    let trait_name = options
        .name
        .unwrap_or_else(|| format_ident!("{model_ident}Scopes", span = model_ident.span()));
    let vis = options
        .vis
        .unwrap_or_else(|| syn::parse_quote!(pub));

    let mut errors = Errors::default();
    let mut methods = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let Some(index) = function.attrs.iter().position(|attr| attr.path().is_ident("scope")) else {
            continue;
        };
        let attr = function.attrs.remove(index);
        if let Err(error) = attr.meta.require_path_only() {
            errors.push(syn::Error::new(error.span(), "#[scope] takes no arguments"));
            continue;
        }
        let signature = &function.sig;
        let name = &signature.ident;
        if signature.asyncness.is_some() {
            errors.push(syn::Error::new(
                signature.asyncness.span(),
                "a scope only builds the query, so it cannot be async",
            ));
            continue;
        }
        let mut inputs = signature.inputs.iter();
        match inputs.next() {
            Some(FnArg::Typed(_)) => {}
            Some(FnArg::Receiver(receiver)) => {
                errors.push(syn::Error::new(
                    receiver.span(),
                    format!(
                        "a scope takes the query, not `self`: `fn {name}(query: Builder<Self>) -> Builder<Self>`"
                    ),
                ));
                continue;
            }
            None => {
                errors.push(syn::Error::new(
                    signature.ident.span(),
                    format!(
                        "a scope takes the query as its first parameter: `fn {name}(query: Builder<Self>) -> Builder<Self>`"
                    ),
                ));
                continue;
            }
        }

        let mut replace = ReplaceSelf {
            model: &model,
            krate: &krate,
        };
        let mut generics = signature.generics.clone();
        replace.visit_generics_mut(&mut generics);
        let (_, _, where_clause) = generics.split_for_impl();
        let params = &generics.params;
        let mut arguments = Vec::new();
        let mut parameters = Vec::new();
        for (position, input) in inputs.enumerate() {
            let FnArg::Typed(typed) = input else {
                continue;
            };
            let argument = format_ident!("scope_argument_{position}", span = Span::call_site());
            let mut ty = (*typed.ty).clone();
            replace.visit_type_mut(&mut ty);
            parameters.push(quote!(#argument: #ty));
            arguments.push(argument);
        }
        let docs: Vec<_> = function
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"))
            .cloned()
            .collect();
        let doc = if docs.is_empty() {
            let text = format!("Applies the `{name}` scope of [`{model_ident}`].");
            quote!(#[doc = #text])
        } else {
            quote!(#(#docs)*)
        };
        let generic_list = if params.is_empty() {
            quote!()
        } else {
            quote!(<#params>)
        };
        methods.push(quote! {
            #doc
            fn #name #generic_list (self, #(#parameters),*) -> Self #where_clause {
                #krate::Scoped::<#model>::apply_scope(self, move |query| {
                    <#model>::#name(query, #(#arguments),*)
                })
            }
        });
    }
    errors.finish()?;
    if methods.is_empty() {
        return Err(syn::Error::new(
            model.span(),
            "#[scopes] found no #[scope] function in this impl block",
        ));
    }

    let trait_doc = format!(
        "The query scopes of [`{model_ident}`], as methods of its query builders (written by `#[scopes]`)."
    );
    Ok(quote! {
        #block

        #[doc = #trait_doc]
        #vis trait #trait_name: #krate::Scoped<#model> + ::core::marker::Sized {
            #(#methods)*
        }

        impl<NavreeScopedBuilder: #krate::Scoped<#model>> #trait_name for NavreeScopedBuilder {}
    })
}
