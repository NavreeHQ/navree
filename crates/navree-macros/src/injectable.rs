//! `#[injectable]`: implements `Injectable` by resolving every field.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::spanned::Spanned;
use syn::{Field, Fields, GenericArgument, Ident, Item, ItemStruct, LitStr, PathArguments, Type};

use crate::attr::{Errors, key_name};
use crate::paths::KERNEL;

/// Expands `#[injectable(args)] item`. On error the item is kept (without
/// its `#[inject]` attributes) so the error does not cascade into "cannot
/// find type" errors wherever the struct is used.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    match try_expand(args, item.clone()) {
        Ok(expanded) => expanded,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(without_inject_attrs(item));
            out
        }
    }
}

fn try_expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let scope = parse_scope(args)?;
    let mut item = parse_struct(item)?;
    let kernel = KERNEL.path(Span::call_site())?;

    let fields = match &mut item.fields {
        Fields::Named(named) => {
            let mut errors = Errors::default();
            let fields: Vec<Injection> = named
                .named
                .iter_mut()
                .filter_map(|field| errors.keep(Injection::from_field(field)))
                .collect();
            errors.finish()?;
            fields
        }
        Fields::Unit => Vec::new(),
        Fields::Unnamed(fields) => {
            return Err(syn::Error::new(
                fields.span(),
                "#[injectable] does not support tuple structs: fields are injected by name; \
                 use named fields (`struct Service { repo: Arc<Repo> }`) or a unit struct",
            ));
        }
    };

    // Mixed-site hygiene: the resolver variable cannot clash with user names.
    let resolver = Ident::new("resolver", Span::mixed_site());
    let deps = fields.iter().filter_map(|f| f.dependency(&kernel));
    let inits = fields.iter().map(|f| f.init(&resolver));
    let construct = match &item.fields {
        Fields::Unit => quote!(Self),
        _ => quote!(Self { #(#inits,)* }),
    };

    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    Ok(quote! {
        #item

        impl #impl_generics #kernel::Injectable for #name #ty_generics #where_clause {
            fn scope() -> #kernel::Scope {
                #kernel::Scope::#scope
            }

            fn dependencies() -> ::std::vec::Vec<#kernel::ProviderKey> {
                ::std::vec![#(#deps),*]
            }

            #[allow(unused_variables)]
            fn construct(
                #resolver: #kernel::Resolver,
            ) -> #kernel::BoxFuture<'static, ::core::result::Result<Self, #kernel::KernelError>> {
                ::std::boxed::Box::pin(async move { ::core::result::Result::Ok(#construct) })
            }
        }
    })
}

/// `item` with the `#[inject]` field attributes removed (when it is a struct).
fn without_inject_attrs(item: TokenStream) -> TokenStream {
    match syn::parse2::<ItemStruct>(item.clone()) {
        Ok(mut item) => {
            for field in item.fields.iter_mut() {
                field.attrs.retain(|attr| !attr.path().is_ident("inject"));
            }
            item.into_token_stream()
        }
        Err(_) => item,
    }
}

/// Parses the annotated item, which must be a struct.
fn parse_struct(item: TokenStream) -> syn::Result<ItemStruct> {
    let span = match syn::parse2::<Item>(item.clone())? {
        Item::Struct(item) => return Ok(item),
        Item::Enum(item) => item.enum_token.span,
        Item::Union(item) => item.union_token.span,
        other => other.span(),
    };
    Err(syn::Error::new(
        span,
        "#[injectable] can only be used on a struct (with named fields, or a unit struct)",
    ))
}

/// How one field is filled.
struct Injection {
    ident: Ident,
    kind: Kind,
}

enum Kind {
    /// `Arc<T>`: the unnamed provider of `T`.
    Provider(Type),
    /// `#[inject(name = "x")] Arc<T>`: the provider of `T` named `x`.
    Named(Type, LitStr),
    /// `Lazy<T>`: resolved on first use; not a dependency edge.
    Lazy(Type),
    /// `#[inject(default)]`: `Default::default()`.
    Default(Span),
}

/// What `#[inject(...)]` on a field asks for.
#[derive(Default)]
struct InjectArgs {
    name: Option<LitStr>,
    /// The span of the `default` flag, when present.
    default: Option<Span>,
}

impl Injection {
    /// Classifies `field` and strips its `#[inject]` attributes.
    fn from_field(field: &mut Field) -> syn::Result<Self> {
        let args = take_inject_args(field)?;
        let ident = field
            .ident
            .clone()
            .ok_or_else(|| syn::Error::new(field.span(), "expected a named field"))?;
        let kind = if args.default.is_some() {
            Kind::Default(field.ty.span())
        } else if let Some(inner) = generic_inner(&field.ty, "Arc") {
            match args.name {
                Some(name) => Kind::Named(inner.clone(), name),
                None => Kind::Provider(inner.clone()),
            }
        } else if let Some(inner) = generic_inner(&field.ty, "Lazy") {
            if let Some(name) = args.name {
                return Err(syn::Error::new(
                    name.span(),
                    "#[inject(name = ...)] cannot be used on a `Lazy<T>` field: lazy handles \
                     resolve the unnamed provider; inject `Arc<T>` instead",
                ));
            }
            Kind::Lazy(inner.clone())
        } else {
            return Err(syn::Error::new(
                field.ty.span(),
                format!(
                    "#[injectable] cannot fill field `{ident}`: use `Arc<T>` for a provider, \
                     `Lazy<T>` for a handle resolved on first use, or mark the field \
                     #[inject(default)] to use `Default::default()`"
                ),
            ));
        };
        Ok(Self { ident, kind })
    }

    /// The provider key this field depends on (none for lazy and default fields).
    fn dependency(&self, kernel: &TokenStream) -> Option<TokenStream> {
        match &self.kind {
            Kind::Provider(ty) => {
                Some(quote_spanned!(ty.span()=> #kernel::ProviderKey::of::<#ty>()))
            }
            Kind::Named(ty, name) => {
                Some(quote_spanned!(ty.span()=> #kernel::ProviderKey::named::<#ty>(#name)))
            }
            Kind::Lazy(_) | Kind::Default(_) => None,
        }
    }

    /// `field: <expression building it with resolver>`.
    fn init(&self, resolver: &Ident) -> TokenStream {
        let ident = &self.ident;
        match &self.kind {
            Kind::Provider(ty) => {
                quote_spanned!(ty.span()=> #ident: #resolver.get::<#ty>().await?)
            }
            Kind::Named(ty, name) => {
                quote_spanned!(ty.span()=> #ident: #resolver.get_named::<#ty>(#name).await?)
            }
            Kind::Lazy(ty) => quote_spanned!(ty.span()=> #ident: #resolver.lazy::<#ty>()),
            Kind::Default(span) => {
                quote_spanned!(*span=> #ident: ::core::default::Default::default())
            }
        }
    }
}

/// Parses `scope = "singleton" | "request" | "transient"` into a `Scope` variant.
fn parse_scope(args: TokenStream) -> syn::Result<Ident> {
    let mut scope: Option<Ident> = None;
    let parser = syn::meta::parser(|meta| {
        if !meta.path.is_ident("scope") {
            return Err(meta.error(format!(
                "unknown #[injectable] argument `{}`: expected \
                 `scope = \"singleton\" | \"request\" | \"transient\"`",
                key_name(&meta.path)
            )));
        }
        if scope.is_some() {
            return Err(meta.error("duplicate `scope` argument"));
        }
        let value: LitStr = meta.value()?.parse()?;
        let variant = match value.value().as_str() {
            "singleton" => "Singleton",
            "request" => "Request",
            "transient" => "Transient",
            other => {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "unknown scope \"{other}\": expected \"singleton\", \"request\" or \"transient\""
                    ),
                ));
            }
        };
        scope = Some(Ident::new(variant, value.span()));
        Ok(())
    });
    syn::parse::Parser::parse2(parser, args)?;
    Ok(scope.unwrap_or_else(|| Ident::new("Singleton", Span::call_site())))
}

/// Parses and removes the `#[inject(...)]` attributes of `field`.
fn take_inject_args(field: &mut Field) -> syn::Result<InjectArgs> {
    let mut args = InjectArgs::default();
    let mut errors = Errors::default();
    field.attrs.retain(|attr| {
        if !attr.path().is_ident("inject") {
            return true;
        }
        errors.keep(attr.parse_nested_meta(|meta| parse_inject_arg(&mut args, &meta)));
        false
    });
    errors.finish()?;
    if let (Some(default), Some(_)) = (args.default, &args.name) {
        return Err(syn::Error::new(
            default,
            "#[inject(default)] cannot be combined with `name`: a defaulted field is not resolved",
        ));
    }
    Ok(args)
}

/// One `name = "..."` or `default` inside `#[inject(...)]`.
fn parse_inject_arg(args: &mut InjectArgs, meta: &ParseNestedMeta) -> syn::Result<()> {
    if meta.path.is_ident("name") {
        if args.name.is_some() {
            return Err(meta.error("duplicate `name` argument"));
        }
        args.name = Some(meta.value()?.parse()?);
    } else if meta.path.is_ident("default") {
        if args.default.is_some() {
            return Err(meta.error("duplicate `default` argument"));
        }
        args.default = Some(meta.path.span());
    } else {
        return Err(meta.error(format!(
            "unknown #[inject] argument `{}`: expected `name = \"...\"` or `default`",
            key_name(&meta.path)
        )));
    }
    Ok(())
}

/// `T` when `ty` is `Wrapper<T>` (any path whose last segment is `wrapper`).
fn generic_inner<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if last.ident != wrapper {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.iter().collect::<Vec<_>>().as_slice() {
        [GenericArgument::Type(inner)] => Some(inner),
        _ => None,
    }
}
