//! `#[relations]`: a model's relation methods, named after themselves and
//! registered by name.
//!
//! Each method of the impl block is a relation method, `fn name(&self) ->
//! RelationType`. Its body is kept, its result named after the method (so
//! `deal.company()` loads under `company`), and `Relationships` is in scope
//! inside it. Two hidden functions are added to the block: the relation of
//! a name (built from a default instance of the model) and the list of
//! names; the model's derived `Model::relation` finds them.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, ItemImpl, ReceiverKind, ReturnType, Type};

use crate::errors::Errors;
use crate::paths;

/// Expands `#[relations]` on an impl block.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(
            args.span(),
            "#[relations] takes no arguments: each method names its relation",
        ));
    }
    let mut block: ItemImpl = syn::parse2(item)?;
    if let Some((path, _)) = &block.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "#[relations] goes on the model's own impl block (`impl Deal { ... }`), not on a trait impl",
        ));
    }
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new(
            block.generics.span(),
            "#[relations] does not take an impl block with generic parameters: a model is one table",
        ));
    }
    let Type::Path(model_path) = &*block.self_ty else {
        return Err(syn::Error::new(
            block.self_ty.span(),
            "#[relations] needs a model type",
        ));
    };
    let Some(model_ident) = model_path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.clone())
    else {
        return Err(syn::Error::new(
            block.self_ty.span(),
            "#[relations] needs a model type",
        ));
    };
    let krate = paths::orm(model_ident.span())?;
    let private = quote!(#krate::__private::relations);

    let mut errors = Errors::default();
    let mut relations = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let signature = &function.sig;
        let name = &signature.ident;
        if signature.asyncness.is_some() {
            errors.push(syn::Error::new(
                signature.asyncness.span(),
                "a relation method only describes the relation, so it cannot be async",
            ));
            continue;
        }
        if !signature.generics.params.is_empty() {
            errors.push(syn::Error::new(
                signature.generics.span(),
                format!("the relation method `{name}` takes no generic parameters"),
            ));
            continue;
        }
        let mut inputs = signature.inputs.iter();
        match inputs.next() {
            Some(FnArg::Receiver(receiver))
                if matches!(receiver.kind, ReceiverKind::Reference(_, _, None)) => {}
            Some(other) => {
                errors.push(syn::Error::new(
                    other.span(),
                    format!(
                        "a relation method takes `&self`: `fn {name}(&self) -> HasMany<Deal, Activity>`"
                    ),
                ));
                continue;
            }
            None => {
                errors.push(syn::Error::new(
                    name.span(),
                    format!(
                        "a relation method takes `&self`: `fn {name}(&self) -> HasMany<Deal, Activity>`"
                    ),
                ));
                continue;
            }
        }
        if let Some(extra) = inputs.next() {
            errors.push(syn::Error::new(
                extra.span(),
                format!(
                    "the relation method `{name}` takes only `&self` (relations are reached by name too, without arguments)"
                ),
            ));
            continue;
        }
        if matches!(signature.output, ReturnType::Default) {
            errors.push(syn::Error::new(
                name.span(),
                format!(
                    "the relation method `{name}` returns its relation: `fn {name}(&self) -> BelongsTo<Deal, Company>`"
                ),
            ));
            continue;
        }
        let relation_name = crate::model::plain_name(name);
        let body = &function.block;
        let output_span = signature.output.span();
        function.block = syn::parse_quote_spanned! {output_span=>
            {
                #[allow(unused_imports)]
                use #krate::relations::Relationships as _;
                #private::named::<Self, _>(#body, #relation_name)
            }
        };
        relations.push((name.clone(), relation_name));
    }
    errors.finish()?;
    if relations.is_empty() {
        return Err(syn::Error::new(
            block.self_ty.span(),
            "#[relations] found no relation method in this impl block",
        ));
    }

    let arms = relations.iter().map(|(method, name)| {
        quote! {
            #name => ::core::option::Option::Some(#private::erased::<Self, _>(
                Self::#method(&template),
            )),
        }
    });
    let names = relations.iter().map(|(_, name)| name);
    let lookup = format_ident!("__navree_relations");
    let list = format_ident!("__navree_relation_names");
    // Relation definitions are read from a default instance: a model
    // without `Default` is reported once, on the impl block's type.
    let self_span = block.self_ty.span();
    let template = quote_spanned! {self_span=> #private::template::<Self>() };
    block.items.push(syn::parse_quote! {
        /// The relation named `name` (written by `#[relations]`).
        #[doc(hidden)]
        pub fn #lookup(name: &str) -> ::core::option::Option<#krate::relations::DynRelation<Self>> {
            if !Self::#list().contains(&name) {
                return ::core::option::Option::None;
            }
            let template: Self = #template;
            match name {
                #(#arms)*
                _ => ::core::option::Option::None,
            }
        }
    });
    block.items.push(syn::parse_quote! {
        /// The names of the relations (written by `#[relations]`).
        #[doc(hidden)]
        pub fn #list() -> &'static [&'static str] {
            &[#(#names),*]
        }
    });
    Ok(quote!(#block))
}
