//! The lifecycle attributes of `#[model(..)]`: `observers(..)`,
//! `dispatches(..)`, `broadcasts`, `prunable` and `mass_prunable`, and the
//! code they add to the expansion.

use proc_macro2::TokenStream;
use quote::quote;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Ident, Token};

/// The lifecycle keys `#[model(..)]` takes on a struct.
pub(crate) const KEYS: &[&str] = &["observers", "prunable", "mass_prunable"];

/// How the model is pruned.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prune {
    /// `prunable`: record by record (`Prunable`).
    Each,
    /// `mass_prunable`: a chunk per statement (`MassPrunable`).
    Mass,
}

/// What the lifecycle attributes say.
#[derive(Default)]
pub(crate) struct Lifecycle {
    /// `observers(DealObserver, ..)`: expressions building observers.
    pub(crate) observers: Vec<syn::Expr>,
    /// `prunable` or `mass_prunable`.
    pub(crate) prune: Option<Prune>,
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
    } else if path.is_ident("prunable") || path.is_ident("mass_prunable") {
        let prune = if path.is_ident("prunable") {
            Prune::Each
        } else {
            Prune::Mass
        };
        if !meta.input.is_empty() && !meta.input.peek(Token![,]) {
            return Err(meta.error(
                "`prunable` and `mass_prunable` take no value: implement `Prunable` / `MassPrunable` for the model",
            ));
        }
        if lifecycle.prune.is_some_and(|known| known != prune) {
            return Err(meta.error(
                "a model is `prunable` (record by record) or `mass_prunable` (a chunk per statement), not both",
            ));
        }
        lifecycle.prune = Some(prune);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// What the lifecycle adds to the model's `ModelMeta`: how it is pruned.
pub(crate) fn meta_additions(lifecycle: &Lifecycle, model: &Ident, krate: &TokenStream) -> TokenStream {
    match lifecycle.prune {
        Some(Prune::Each) => quote!(.pruned(#krate::prune::Pruning::each::<#model>())),
        Some(Prune::Mass) => quote!(.pruned(#krate::prune::Pruning::mass::<#model>())),
        None => quote!(),
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

/// The capability traits a model's declarations give it:
/// `SoftDeleting` for `soft_deletes`.
pub(crate) fn capability_impls(model: &Ident, soft_deletes: bool, krate: &TokenStream) -> TokenStream {
    let mut impls = TokenStream::new();
    if soft_deletes {
        impls.extend(quote! {
            #[automatically_derived]
            impl #krate::SoftDeleting for #model {}
        });
    }
    impls
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
