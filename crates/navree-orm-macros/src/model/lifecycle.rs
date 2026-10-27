//! The lifecycle attributes of `#[model(..)]`: `observers(..)`,
//! `dispatches(..)`, `broadcasts`, `prunable` and `mass_prunable`, and the
//! code they add to the expansion.

use proc_macro2::TokenStream;
use quote::quote;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Ident, Token};

/// The lifecycle keys `#[model(..)]` takes on a struct.
pub(crate) const KEYS: &[&str] = &["observers"];

/// What the lifecycle attributes say.
#[derive(Default)]
pub(crate) struct Lifecycle {
    /// `observers(DealObserver, ..)`: expressions building observers.
    pub(crate) observers: Vec<syn::Expr>,
}

/// Parses `meta` when it is a lifecycle key; `Ok(false)` when it is not.
pub(crate) fn parse_key(lifecycle: &mut Lifecycle, meta: &ParseNestedMeta<'_>) -> syn::Result<bool> {
    let path = &meta.path;
    if path.is_ident("observers") {
        let content;
        syn::parenthesized!(content in meta.input);
        let observers = Punctuated::<syn::Expr, Token![,]>::parse_terminated(&content)?;
        if observers.is_empty() {
            return Err(meta.error("observers(..) needs at least one observer, e.g. observers(DealObserver)"));
        }
        lifecycle.observers.extend(observers);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// The entries of the model's `hooks()`: one `Arc<dyn ModelHook<Self>>`
/// per declared observer.
pub(crate) fn hook_entries(lifecycle: &Lifecycle, krate: &TokenStream) -> Vec<TokenStream> {
    let private = quote!(#krate::__private);
    lifecycle
        .observers
        .iter()
        .map(|observer| {
            quote!(#private::Arc::new(#krate::ObserverHook::new(#observer))
                as #private::Arc<dyn #krate::hooks::ModelHook<Self>>)
        })
        .collect()
}

/// `fn hooks()`, built once per model (in a `static`) from `entries`.
pub(crate) fn hooks_fn(model: &Ident, entries: &[TokenStream], krate: &TokenStream) -> TokenStream {
    if entries.is_empty() {
        return quote!();
    }
    let private = quote!(#krate::__private);
    quote! {
        fn hooks() -> ::std::vec::Vec<#private::Arc<dyn #krate::hooks::ModelHook<Self>>> {
            static HOOKS: #private::OnceLock<::std::vec::Vec<#private::Arc<dyn #krate::hooks::ModelHook<#model>>>> =
                #private::OnceLock::new();
            ::core::clone::Clone::clone(HOOKS.get_or_init(|| ::std::vec![#(#entries),*]))
        }
    }
}
