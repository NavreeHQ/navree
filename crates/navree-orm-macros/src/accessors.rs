//! `#[accessors]`: the `#[accessor]` methods of an impl block become the
//! model's accessors, which serialized records can append.
//!
//! The methods stay as they are (minus the attribute); the block gains a
//! hidden constant, `__NAVREE_ACCESSORS`, listing them with how to read
//! each as JSON. `#[derive(Model)]` reads that constant (an inherent
//! constant takes precedence over the empty default every model has), so
//! the derive and this attribute need not see each other.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, ItemImpl, LitStr, ReturnType, Type};

use crate::errors::{Errors, key_name};
use crate::model::plain_name;
use crate::paths;

/// `#[accessor]` or `#[accessor(name = "...", schema = Type)]`.
#[derive(Default)]
struct AccessorArgs {
    name: Option<LitStr>,
    schema: Option<Type>,
}

fn parse_args(attr: &syn::Attribute) -> syn::Result<AccessorArgs> {
    let mut args = AccessorArgs::default();
    if matches!(attr.meta, syn::Meta::Path(_)) {
        return Ok(args);
    }
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("name") {
            let name: LitStr = meta.value()?.parse()?;
            if name.value().trim().is_empty() {
                return Err(syn::Error::new(name.span(), "an accessor's name cannot be empty"));
            }
            args.name = Some(name);
            Ok(())
        } else if meta.path.is_ident("schema") {
            let value = meta.value()?;
            args.schema = Some(if value.peek(LitStr) {
                value.parse::<LitStr>()?.parse()?
            } else {
                value.parse()?
            });
            Ok(())
        } else {
            Err(meta.error(format!(
                "unknown accessor option `{}`; expected `name` (its key in serialized records) or `schema` (the OpenAPI type of its value)",
                key_name(&meta.path)
            )))
        }
    })?;
    Ok(args)
}

/// The impl block without its `#[accessor]` markers (what stays of it when
/// `#[accessors]` reports an error); nothing when it is not an impl block.
pub(crate) fn without_markers(item: TokenStream) -> TokenStream {
    let Ok(mut block) = syn::parse2::<ItemImpl>(item) else {
        return TokenStream::new();
    };
    for item in &mut block.items {
        if let ImplItem::Fn(function) = item {
            function.attrs.retain(|attr| !attr.path().is_ident("accessor"));
        }
    }
    quote!(#block)
}

/// The return type as written, without the token spacing.
fn type_text(ty: &Type) -> String {
    quote!(#ty)
        .to_string()
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
        .replace("& ", "&")
}

/// Expands `#[accessors]` on an impl block.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(
            args.span(),
            "#[accessors] takes no arguments: mark each accessor with #[accessor]",
        ));
    }
    let mut block: ItemImpl = syn::parse2(item)?;
    if let Some((path, _)) = &block.trait_ {
        return Err(syn::Error::new(
            path.span(),
            "#[accessors] goes on the model's own impl block (`impl Deal { ... }`), not on a trait impl",
        ));
    }
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new(
            block.generics.span(),
            "#[accessors] does not take an impl block with generic parameters",
        ));
    }
    let span = block.self_ty.span();
    let krate = paths::orm(span)?;

    let mut errors = Errors::default();
    let mut entries = Vec::new();
    let mut names: Vec<(String, Span)> = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        let Some(index) = function
            .attrs
            .iter()
            .position(|attr| attr.path().is_ident("accessor"))
        else {
            continue;
        };
        let attr = function.attrs.remove(index);
        let args = match parse_args(&attr) {
            Ok(args) => args,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let signature = &function.sig;
        let method = &signature.ident;
        let usage = format!("an accessor reads the model: `fn {method}(&self) -> T`");
        if signature.asyncness.is_some() {
            errors.push(syn::Error::new(
                signature.asyncness.span(),
                format!("{usage}, computed from its fields, so it cannot be async"),
            ));
            continue;
        }
        if !signature.generics.params.is_empty() {
            errors.push(syn::Error::new(
                signature.generics.span(),
                format!("{usage}, without generic parameters"),
            ));
            continue;
        }
        let mut inputs = signature.inputs.iter();
        match inputs.next() {
            Some(FnArg::Receiver(syn::Receiver {
                kind: syn::ReceiverKind::Reference(_, _, None),
                ..
            })) => {}
            Some(other) => {
                errors.push(syn::Error::new(other.span(), usage));
                continue;
            }
            None => {
                errors.push(syn::Error::new(method.span(), usage));
                continue;
            }
        }
        if let Some(extra) = inputs.next() {
            errors.push(syn::Error::new(
                extra.span(),
                format!("{usage}, with no other parameters"),
            ));
            continue;
        }
        let ReturnType::Type(_, return_type) = &signature.output else {
            errors.push(syn::Error::new(
                method.span(),
                format!("{usage}: it returns the value serialized records append"),
            ));
            continue;
        };
        let name = args
            .name
            .as_ref()
            .map_or_else(|| plain_name(method), LitStr::value);
        let name_span = args.name.as_ref().map_or_else(|| method.span(), LitStr::span);
        if let Some((_, first)) = names.iter().find(|(known, _)| *known == name) {
            let mut error = syn::Error::new(name_span, format!("two accessors are named `{name}`"));
            error.combine(syn::Error::new(*first, "the first one"));
            errors.push(error);
            continue;
        }
        names.push((name.clone(), name_span));
        let rust_type = type_text(return_type);
        let mut entry = quote! {
            #krate::attributes::Accessor::new(
                #name,
                |model: &Self| #krate::__private::accessor_json(&Self::#method(model)),
                #rust_type,
            )
        };
        if let Some(schema) = &args.schema {
            entry = quote!(#entry.with_schema(#krate::__private::schema_json::<#schema>));
        }
        entries.push(entry);
    }
    errors.finish()?;
    if entries.is_empty() {
        return Err(syn::Error::new(
            span,
            "#[accessors] found no #[accessor] method in this impl block",
        ));
    }
    block.items.push(syn::parse_quote! {
        /// The accessors of this model (written by `#[accessors]`).
        #[doc(hidden)]
        pub const __NAVREE_ACCESSORS: &'static [#krate::attributes::Accessor<Self>] = &[#(#entries),*];
    });
    Ok(quote!(#block))
}
