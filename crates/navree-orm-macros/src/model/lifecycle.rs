//! The lifecycle attributes of `#[model(..)]`: `observers(..)`,
//! `dispatches(..)`, `broadcasts`, `prunable` and `mass_prunable`, and the
//! code they add to the expansion.

use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Ident, Token};

use crate::errors::{key_name, suggestion};

/// The lifecycle keys `#[model(..)]` takes on a struct.
pub(crate) const KEYS: &[&str] = &[
    "observers",
    "dispatches",
    "broadcasts",
    "prunable",
    "mass_prunable",
];

/// The model events, as `dispatches(..)` names them.
const EVENTS: &[&str] = &[
    "retrieved",
    "saving",
    "creating",
    "created",
    "updating",
    "updated",
    "saved",
    "deleting",
    "deleted",
    "trashed",
    "force_deleting",
    "force_deleted",
    "restoring",
    "restored",
    "replicating",
];

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
    /// `dispatches(created = DealCreated, ..)`: the `ModelEvent` variant
    /// and the application event type.
    pub(crate) dispatches: Vec<(Ident, syn::Type)>,
    /// `broadcasts`: changes go out on real-time channels.
    pub(crate) broadcasts: bool,
    /// `prunable` or `mass_prunable`.
    pub(crate) prune: Option<Prune>,
}

/// `Ok` when the flag `meta` stands alone (no `= value`, no list).
fn flag(meta: &ParseNestedMeta<'_>, what: &str) -> syn::Result<()> {
    if meta.input.is_empty() || meta.input.peek(Token![,]) {
        Ok(())
    } else {
        Err(meta.error(format!("`{}` takes no value: {what}", key_name(&meta.path))))
    }
}

/// Parses `meta` when it is a lifecycle key; `Ok(false)` when it is not.
pub(crate) fn parse_key(
    lifecycle: &mut Lifecycle,
    meta: &ParseNestedMeta<'_>,
) -> syn::Result<bool> {
    let path = &meta.path;
    if path.is_ident("observers") {
        let content;
        syn::parenthesized!(content in meta.input);
        let observers = Punctuated::<syn::Expr, Token![,]>::parse_terminated(&content)?;
        if observers.is_empty() {
            return Err(meta
                .error("observers(..) needs at least one observer, e.g. observers(DealObserver)"));
        }
        lifecycle.observers.extend(observers);
        Ok(true)
    } else if path.is_ident("dispatches") {
        meta.parse_nested_meta(|inner| {
            let name = key_name(&inner.path);
            if !EVENTS.contains(&name.as_str()) {
                let hint = suggestion(&name, EVENTS)
                    .map(|known| format!(" (did you mean `{known}`?)"))
                    .unwrap_or_default();
                return Err(inner.error(format!(
                    "unknown model event `{name}`{hint}; expected one of: {}",
                    EVENTS.join(", ")
                )));
            }
            let event: syn::Type = inner.value()?.parse()?;
            let variant = Ident::new(&name.to_upper_camel_case(), inner.path.span());
            lifecycle.dispatches.push((variant, event));
            Ok(())
        })?;
        Ok(true)
    } else if path.is_ident("broadcasts") {
        flag(
            meta,
            "implement `Broadcasts` for the model to choose channels, names, data and a queue",
        )?;
        lifecycle.broadcasts = true;
        Ok(true)
    } else if path.is_ident("prunable") || path.is_ident("mass_prunable") {
        let prune = if path.is_ident("prunable") {
            Prune::Each
        } else {
            Prune::Mass
        };
        flag(
            meta,
            "implement `Prunable` / `MassPrunable` for the model to say which records go",
        )?;
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
pub(crate) fn meta_additions(
    lifecycle: &Lifecycle,
    model: &Ident,
    krate: &TokenStream,
) -> TokenStream {
    match lifecycle.prune {
        Some(Prune::Each) => quote!(.pruned(#krate::prune::Pruning::each::<#model>())),
        Some(Prune::Mass) => quote!(.pruned(#krate::prune::Pruning::mass::<#model>())),
        None => quote!(),
    }
}

/// The entries of the model's `hooks()`: one `Arc<dyn ModelHook<Self>>`
/// per declared observer and dispatched event.
pub(crate) fn hook_entries(lifecycle: &Lifecycle, krate: &TokenStream) -> Vec<TokenStream> {
    let private = quote!(#krate::__private);
    let observers = lifecycle.observers.iter().map(|observer| {
        quote!(#private::Arc::new(#krate::ObserverHook::new(#observer))
            as #private::Arc<dyn #krate::hooks::ModelHook<Self>>)
    });
    let dispatches = lifecycle.dispatches.iter().map(|(variant, event)| {
        quote!(#private::Arc::new(
            #krate::events::Dispatch::<Self, #event>::new(#krate::hooks::ModelEvent::#variant)
        ) as #private::Arc<dyn #krate::hooks::ModelHook<Self>>)
    });
    let broadcasts = lifecycle.broadcasts.then(|| {
        quote!(#private::Arc::new(#krate::broadcast::BroadcastHook::<Self>::new())
            as #private::Arc<dyn #krate::hooks::ModelHook<Self>>)
    });
    observers.chain(dispatches).chain(broadcasts).collect()
}

/// The capability traits a model's declarations give it:
/// `SoftDeleting` for `soft_deletes`.
pub(crate) fn capability_impls(
    model: &Ident,
    soft_deletes: bool,
    krate: &TokenStream,
) -> TokenStream {
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
