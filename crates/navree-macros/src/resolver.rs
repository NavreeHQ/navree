//! `#[resolver]`: implements `navree_graphql::GraphqlResolver` for an impl
//! block whose methods carry `#[query]`, `#[mutation]` or `#[subscription]`.
//!
//! The impl block stays as written (minus the helper attributes). Next to
//! it, inside a `const _: () = { .. };` block, the expansion defines one
//! async-graphql object per kind (`#[Object]` for queries and mutations,
//! `#[Subscription]` for subscriptions) holding an `Arc` of the resolver.
//! Each generated field checks the access declaration, runs the method in
//! the field's unit of work and validates `Valid<T>` arguments, then calls
//! the method; `GraphqlResolver::parts` hands the objects to the schema.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{
    Attribute, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, Item, ItemImpl, LitStr, Pat,
    Path, PathArguments, ReceiverKind, ReturnType, Type,
};

use crate::attr::Errors;
use crate::paths::GRAPHQL;
use crate::route::{Access, At, Can, Name, flag, strings};

/// What kind of root field a method is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Query,
    Mutation,
    Subscription,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Query, Kind::Mutation, Kind::Subscription];

    fn name(self) -> &'static str {
        match self {
            Kind::Query => "Query",
            Kind::Mutation => "Mutation",
            Kind::Subscription => "Subscription",
        }
    }
}

/// One of the helper attributes `#[resolver]` understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attr {
    Field(Kind),
    Public,
    Authenticated,
    Roles,
    Permissions,
    Can,
}

impl Attr {
    /// The attribute `attr` is, when it is one of ours: a plain name
    /// (`#[query]`) or one qualified by the facade or this crate.
    fn of(attr: &Attribute) -> Option<Attr> {
        let path = attr.path();
        let ours = match path.segments.len() {
            1 => path.leading_colon.is_none(),
            2 => matches!(
                path.segments[0].ident.to_string().as_str(),
                "navree" | "navree_macros"
            ),
            _ => false,
        };
        if !ours {
            return None;
        }
        Some(match path.segments.last()?.ident.to_string().as_str() {
            "query" => Attr::Field(Kind::Query),
            "mutation" => Attr::Field(Kind::Mutation),
            "subscription" => Attr::Field(Kind::Subscription),
            "public" => Attr::Public,
            "authenticated" => Attr::Authenticated,
            "roles" => Attr::Roles,
            "permissions" => Attr::Permissions,
            "can" => Attr::Can,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Attr::Field(Kind::Query) => "query",
            Attr::Field(Kind::Mutation) => "mutation",
            Attr::Field(Kind::Subscription) => "subscription",
            Attr::Public => "public",
            Attr::Authenticated => "authenticated",
            Attr::Roles => "roles",
            Attr::Permissions => "permissions",
            Attr::Can => "can",
        }
    }
}

/// The helper attributes of one method or of the impl block.
#[derive(Default)]
struct FieldAttrs {
    kind: Option<(Kind, At)>,
    access: Access,
    /// The first access attribute, for "only applies to fields" errors.
    first_access: Option<(&'static str, At)>,
}

/// Where the attributes were written.
#[derive(Clone, Copy)]
enum Site<'a> {
    Impl,
    Method(&'a Ident),
}

impl FieldAttrs {
    /// Removes our attributes from `attrs` and parses them; problems go to
    /// `errors`.
    fn take(attrs: &mut Vec<Attribute>, site: Site, errors: &mut Errors) -> Self {
        let mut parsed = FieldAttrs::default();
        let mut seen: Vec<Attr> = Vec::new();
        attrs.retain(|attr| {
            let Some(kind) = Attr::of(attr) else {
                return true;
            };
            let at = At::of(attr);
            let name = kind.name();
            if let (Site::Impl, Attr::Field(_)) = (site, kind) {
                errors.push(at.error(format!(
                    "#[{name}] goes on a method of the impl block, not on the impl block"
                )));
                return false;
            }
            let repeatable = matches!(kind, Attr::Roles | Attr::Permissions);
            let same_slot = |other: &Attr| {
                *other == kind || matches!((other, kind), (Attr::Field(_), Attr::Field(_)))
            };
            if !repeatable && let Some(first) = seen.iter().find(|other| same_slot(other)) {
                let message = match (first, site) {
                    (Attr::Field(_), Site::Method(method)) => format!(
                        "a method is one field: `{method}` already has #[{}]",
                        first.name()
                    ),
                    _ => format!("duplicate #[{name}]"),
                };
                errors.push(at.error(message));
                return false;
            }
            seen.push(kind);
            if !matches!(kind, Attr::Field(_)) && parsed.first_access.is_none() {
                parsed.first_access = Some((name, at.clone()));
            }
            if let Err(error) = parsed.parse(kind, attr, at) {
                if !matches!(kind, Attr::Field(_)) {
                    parsed.access.malformed = true;
                }
                errors.push(error);
            }
            false
        });
        parsed
    }

    fn parse(&mut self, kind: Attr, attr: &Attribute, at: At) -> syn::Result<()> {
        match kind {
            Attr::Field(field) => {
                flag(attr)?;
                self.kind = Some((field, at));
            }
            Attr::Public => self.access.public = Some(flag(attr)?),
            Attr::Authenticated => self.access.authenticated = Some(flag(attr)?),
            Attr::Roles => {
                let roles = strings(attr, "\"admin\", \"manager\"")?;
                self.access.roles.extend(roles);
                self.access.roles_at.get_or_insert(at);
            }
            Attr::Permissions => {
                let permissions = strings(attr, "\"deals.update\"")?;
                self.access.permissions.extend(permissions);
                self.access.permissions_at.get_or_insert(at);
            }
            Attr::Can => {
                let can = Can::parse_args(attr, &at)?;
                self.access.can = Some((can, at));
            }
        }
        Ok(())
    }

    /// Adds the impl block's access declarations to a field's own.
    fn inherit(&mut self, shared: &Access) {
        let access = &mut self.access;
        if access.public.is_none() {
            access.public.clone_from(&shared.public);
        }
        if access.authenticated.is_none() {
            access.authenticated.clone_from(&shared.authenticated);
        }
        access.roles.splice(0..0, shared.roles.iter().cloned());
        if access.roles_at.is_none() {
            access.roles_at.clone_from(&shared.roles_at);
        }
        access
            .permissions
            .splice(0..0, shared.permissions.iter().cloned());
        if access.permissions_at.is_none() {
            access.permissions_at.clone_from(&shared.permissions_at);
        }
        if access.can.is_none() {
            access.can.clone_from(&shared.can);
        }
    }
}

/// A GraphQL argument of a field method.
struct Param {
    ident: Ident,
    ty: Type,
    /// The `#[graphql(..)]` attributes, moved to the generated field.
    attrs: Vec<Attribute>,
    /// For `Valid<T>`: the constructor path (`Valid`) and `T`.
    valid: Option<(Path, Type)>,
}

/// One field method.
struct FieldFn {
    ident: Ident,
    kind: Kind,
    access: Access,
    /// `#[doc]` (descriptions), `#[graphql(..)]` and `#[cfg]` attributes
    /// for the generated field.
    attrs: Vec<Attribute>,
    /// Whether the method takes the GraphQL context after `&self`.
    has_ctx: bool,
    params: Vec<Param>,
    /// The value type (`T` of `Result<T>`), and whether the method returns
    /// a `Result`.
    value: Type,
    is_result: bool,
}

/// The end of the "no access declaration" error.
const DECLARE_ACCESS: &str =
    "add #[public], #[authenticated], #[roles], #[permissions] or #[can] (deny by default)";

/// The end of the "public and restricted" error.
const PUBLIC_CONFLICT: &str =
    "a public field needs no authentication or authorization; remove one of them";

/// Expands `#[resolver] impl X { .. }`.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let mut item = match parse_impl(item.clone()) {
        Ok(item) => item,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(item);
            return out;
        }
    };
    let mut errors = Errors::default();
    if !args.is_empty() {
        errors.push(syn::Error::new_spanned(
            &args,
            "#[resolver] takes no arguments",
        ));
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &item.generics,
            "a #[resolver] impl block cannot be generic: the schema needs one concrete type",
        ));
    }
    let shared = FieldAttrs::take(&mut item.attrs, Site::Impl, &mut errors);
    if let (Some(_), Some((restriction, at))) = (&shared.access.public, shared.access.restriction())
    {
        errors.push(at.error(format!(
            "the impl block is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
        )));
    }
    let fields = collect_fields(&mut item, &shared.access, &mut errors);
    if fields.is_empty() && !has_field_attempt(&errors) {
        errors.push(syn::Error::new_spanned(
            &item.self_ty,
            "a #[resolver] impl block needs at least one method marked #[query], #[mutation] or \
             #[subscription]",
        ));
    }
    let graphql = errors.keep(GRAPHQL.path(Span::call_site()));
    let failed = errors.finish();
    let mut out = TokenStream::new();
    match (failed, graphql) {
        (Ok(()), Some(graphql)) => out.extend(generate(&item, &fields, &graphql)),
        (Err(error), _) => {
            out.extend(error.into_compile_error());
            out.extend(item.into_token_stream());
        }
        (Ok(()), None) => out.extend(item.into_token_stream()),
    }
    out
}

/// Whether some error was already reported (so "no fields" would only add
/// noise).
fn has_field_attempt(errors: &Errors) -> bool {
    errors.has_errors()
}

/// Parses the annotated item, which must be an inherent impl block.
fn parse_impl(item: TokenStream) -> syn::Result<ItemImpl> {
    match syn::parse2::<Item>(item)? {
        Item::Impl(item) => match &item.trait_ {
            None => Ok(item),
            Some((path, _)) => Err(syn::Error::new_spanned(
                path,
                "#[resolver] goes on an inherent impl block (`impl DealResolver { .. }`), not on \
                 a trait impl",
            )),
        },
        other => Err(syn::Error::new(
            other.span(),
            "#[resolver] goes on an impl block: `#[resolver] impl DealResolver { .. }`",
        )),
    }
}

/// Strips the helper attributes from the methods of `item` and collects
/// the fields, each with the impl block's `shared` access added.
fn collect_fields(item: &mut ItemImpl, shared: &Access, errors: &mut Errors) -> Vec<FieldFn> {
    let mut fields = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let ident = method.sig.ident.clone();
        let mut attrs = FieldAttrs::take(&mut method.attrs, Site::Method(&ident), errors);
        let Some((kind, _)) = attrs.kind.take() else {
            if let Some((name, at)) = &attrs.first_access {
                errors.push(at.error(format!(
                    "#[{name}] only applies to fields: `{ident}` has no #[query], #[mutation] or \
                     #[subscription]"
                )));
            }
            strip_graphql_param_attrs(method);
            continue;
        };
        if let Some((restriction, at)) = attrs.access.public_conflict(shared) {
            errors.push(at.error(format!(
                "field `{ident}` is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
            )));
        }
        if let (Some((_, at)), Some(_)) = (&attrs.access.can, &shared.can) {
            errors.push(at.error(format!(
                "field `{ident}` has #[can] here and on the impl block: a field checks one \
                 ability; move the impl block's #[can] to the fields that need it"
            )));
        }
        attrs.inherit(shared);
        if !attrs.access.is_declared() && !attrs.access.malformed {
            errors.push(syn::Error::new(
                ident.span(),
                format!("field `{ident}` has no access declaration: {DECLARE_ACCESS}"),
            ));
        }
        match field_fn(method, kind, attrs.access) {
            Ok(field) => {
                if let Some((can, _)) = &field.access.can {
                    errors.keep(check_from(can, &field));
                }
                fields.push(field);
            }
            Err(error) => {
                errors.push(error);
                strip_graphql_param_attrs(method);
            }
        }
    }
    fields
}

/// Removes `#[graphql(..)]` from the parameters of a method left as is.
fn strip_graphql_param_attrs(method: &mut ImplItemFn) {
    for input in &mut method.sig.inputs {
        if let FnArg::Typed(arg) = input {
            arg.attrs.retain(|attr| !attr.path().is_ident("graphql"));
        }
    }
}

/// Reads a field method, moving its `#[graphql]` attributes (on the method
/// and its parameters) to the generated field.
fn field_fn(method: &mut ImplItemFn, kind: Kind, access: Access) -> syn::Result<FieldFn> {
    check_signature(method)?;
    let ident = method.sig.ident.clone();
    let mut attrs = Vec::new();
    method.attrs.retain(|attr| {
        if attr.path().is_ident("graphql") {
            attrs.push(attr.clone());
            false
        } else {
            if attr.path().is_ident("doc") || attr.path().is_ident("cfg") {
                attrs.push(attr.clone());
            }
            true
        }
    });
    let mut has_ctx = false;
    let mut params = Vec::new();
    for (index, input) in method.sig.inputs.iter_mut().enumerate() {
        let FnArg::Typed(arg) = input else {
            continue;
        };
        if index == 1 && is_context(&arg.ty) {
            has_ctx = true;
            continue;
        }
        let Pat::Ident(pat) = &*arg.pat else {
            return Err(syn::Error::new_spanned(
                &arg.pat,
                format!(
                    "the arguments of field `{ident}` must be plain names (`id: Uuid`): each is a \
                     GraphQL argument"
                ),
            ));
        };
        let mut graphql_attrs = Vec::new();
        arg.attrs.retain(|attr| {
            if attr.path().is_ident("graphql") {
                graphql_attrs.push(attr.clone());
                false
            } else {
                true
            }
        });
        params.push(Param {
            ident: pat.ident.clone(),
            ty: (*arg.ty).clone(),
            attrs: graphql_attrs,
            valid: valid_inner(&arg.ty),
        });
    }
    let (value, is_result) = match &method.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new(
                ident.span(),
                format!("field `{ident}` must return a value (`-> Result<T>`)"),
            ));
        }
        ReturnType::Type(_, ty) => match result_inner(ty) {
            Some(inner) => (inner.clone(), true),
            None => ((**ty).clone(), false),
        },
    };
    Ok(FieldFn {
        ident,
        kind,
        access,
        attrs,
        has_ctx,
        params,
        value,
        is_result,
    })
}

/// A field method is a non-generic `async fn` taking `&self`.
fn check_signature(method: &ImplItemFn) -> syn::Result<()> {
    let sig = &method.sig;
    let ident = &sig.ident;
    let mut errors = Errors::default();
    if sig.asyncness.is_none() {
        errors.push(syn::Error::new(
            ident.span(),
            format!("field `{ident}` must be an `async fn`"),
        ));
    }
    let receiver_error = format!(
        "field `{ident}` must take `&self`: the generated field calls it on the shared resolver"
    );
    match sig.receiver() {
        Some(receiver) => {
            if !matches!(receiver.kind, ReceiverKind::Reference(_, _, None)) {
                errors.push(syn::Error::new_spanned(receiver, receiver_error));
            }
        }
        None => errors.push(syn::Error::new(ident.span(), receiver_error)),
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &sig.generics,
            format!("field `{ident}` cannot be generic: the schema needs concrete types"),
        ));
    }
    errors.finish()
}

/// `from = "id"` must name an argument of the field (the record id comes
/// from it).
fn check_from(can: &Can, field: &FieldFn) -> syn::Result<()> {
    let Some(from) = &can.from else {
        return Ok(());
    };
    let name = from.value();
    if field.params.iter().any(|param| param.ident == name) {
        return Ok(());
    }
    Err(syn::Error::new(
        from.span(),
        format!(
            "`from = \"{name}\"` names no argument of field `{}`: the record id must come from an \
             argument such as `{name}: Uuid`",
            field.ident
        ),
    ))
}

/// Whether `ty` is `&Context<'_>` (any path ending in `Context`).
fn is_context(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Path(path) = &*reference.elem else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Context")
}

/// `T` of `<path>::Result<T, ..>`.
fn result_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

/// For `Valid<T>` (any path ending in `Valid` with one type argument): the
/// path without arguments, to construct it, and `T`.
fn valid_inner(ty: &Type) -> Option<(Path, Type)> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if last.ident != "Valid" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    let inner = match args.args.iter().collect::<Vec<_>>().as_slice() {
        [GenericArgument::Type(inner)] => inner.clone(),
        _ => return None,
    };
    let mut ctor = path.path.clone();
    if let Some(segment) = ctor.segments.last_mut() {
        segment.arguments = PathArguments::None;
    }
    Some((ctor, inner))
}

/// The impl block, then the generated objects and `impl GraphqlResolver`.
fn generate(item: &ItemImpl, fields: &[FieldFn], graphql: &TokenStream) -> TokenStream {
    let self_ty = &item.self_ty;
    let ag = quote!(#graphql::async_graphql);
    let private = quote!(#graphql::__private);
    let crate_path = LitStr::new(&ag.to_string(), Span::call_site());

    let mut objects = TokenStream::new();
    let mut parts = TokenStream::new();
    for kind in Kind::ALL {
        let kind_fields: Vec<&FieldFn> = fields.iter().filter(|f| f.kind == kind).collect();
        let slot = format_ident!("{}", kind.name().to_ascii_lowercase());
        if kind_fields.is_empty() {
            parts.extend(quote!(#slot: ::core::option::Option::None,));
            continue;
        }
        let object = format_ident!("__Navree{}", kind.name());
        let kind_name = kind.name();
        let derive = match kind {
            Kind::Subscription => quote!(#ag::Subscription),
            Kind::Query | Kind::Mutation => quote!(#ag::Object),
        };
        let erased = match kind {
            Kind::Subscription => quote!(#private::ErasedSubscription),
            Kind::Query | Kind::Mutation => quote!(#private::ErasedObject),
        };
        let methods = kind_fields
            .iter()
            .map(|field| field_method(field, &ag, &private));
        objects.extend(quote! {
            struct #object(::std::sync::Arc<#self_ty>);

            impl #ag::TypeName for #object {
                fn type_name() -> ::std::borrow::Cow<'static, str> {
                    #private::part_type_name::<#self_ty>(#kind_name)
                }
            }

            #[#derive(crate = #crate_path, name_type)]
            impl #object {
                #(#methods)*
            }
        });
        parts.extend(quote! {
            #slot: ::core::option::Option::Some(
                ::std::sync::Arc::new(#object(::std::sync::Arc::clone(&self)))
                    as ::std::sync::Arc<dyn #erased>
            ),
        });
    }

    let specs_var = Ident::new("specs", Span::mixed_site());
    let pushes = fields.iter().map(|field| {
        let name = LitStr::new(&field.ident.to_string(), field.ident.span());
        let kind = format_ident!("{}", field.kind.name());
        let access = access_value(&field.access, &private);
        let cfgs = field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("cfg"));
        quote! {
            #(#cfgs)*
            #specs_var.push(#private::FieldSpec {
                name: #name,
                kind: #private::FieldKind::#kind,
                access: #access,
            });
        }
    });
    let specs = quote! {
        let mut #specs_var = ::std::vec::Vec::new();
        #(#pushes)*
        #specs_var
    };

    quote! {
        #item

        const _: () = {
            #objects

            impl #graphql::GraphqlResolver for #self_ty {
                fn parts(self: ::std::sync::Arc<Self>) -> #private::ResolverParts {
                    #private::ResolverParts {
                        #parts
                        fields: { #specs },
                    }
                }
            }
        };
    }
}

/// The generated async-graphql field of `field`.
fn field_method(field: &FieldFn, ag: &TokenStream, private: &TokenStream) -> TokenStream {
    let ident = &field.ident;
    let attrs = &field.attrs;
    let ctx = Ident::new("ctx", Span::mixed_site());
    let guard = Ident::new("field", Span::mixed_site());
    let access_const = Ident::new("ACCESS", Span::mixed_site());
    let kind = format_ident!("{}", field.kind.name());
    let value = &field.value;
    let access = access_value(&field.access, private);

    let params = field.params.iter().map(|param| {
        let Param {
            ident, ty, attrs, ..
        } = param;
        match &param.valid {
            Some((_, inner)) => {
                quote_spanned!(ty.span()=> #(#attrs)* #ident: #private::ValidArg<#inner>)
            }
            None => quote_spanned!(ty.span()=> #(#attrs)* #ident: #ty),
        }
    });
    let args = match field
        .access
        .can
        .as_ref()
        .and_then(|(can, _)| can.from.as_ref())
    {
        Some(from) => {
            let param = Ident::new(&from.value(), from.span());
            quote!(&[(#from, #private::arg_text(&#param))])
        }
        None => quote!(&[]),
    };
    let validations = field
        .params
        .iter()
        .filter(|p| p.valid.is_some())
        .map(|param| {
            let ident = &param.ident;
            let name = LitStr::new(&ident.to_string(), ident.span());
            quote!(let #ident = #private::validate(#ctx, #name, #ident)?;)
        });
    let mut call_args = Vec::new();
    if field.has_ctx {
        call_args.push(quote!(#ctx));
    }
    for param in &field.params {
        let ident = &param.ident;
        call_args.push(match &param.valid {
            Some((ctor, _)) => quote!(#ctor(#ident)),
            None => quote!(#ident),
        });
    }
    let call = quote!(self.0.#ident(#(#call_args),*).await);
    let call = if field.is_result {
        quote!(#call.map_err(::core::convert::Into::<#ag::Error>::into))
    } else {
        quote!(::core::result::Result::Ok(#call))
    };
    quote! {
        #(#attrs)*
        async fn #ident(&self, #ctx: &#ag::Context<'_>, #(#params),*) -> #ag::Result<#value> {
            const #access_const: #private::FieldAccess = #access;
            let #guard = #private::Field::begin(
                #ctx,
                #private::FieldKind::#kind,
                &#access_const,
                #args,
            )
            .await?;
            #guard
                .run(async move {
                    #(#validations)*
                    #call
                })
                .await
        }
    }
}

/// The `FieldAccess` value of a declaration: marker types become their
/// constants (`<Update as ActionMarker>::NAME`, `<Deal as Subject>::SUBJECT`).
fn access_value(access: &Access, private: &TokenStream) -> TokenStream {
    let public = access.public.is_some();
    let authenticated = access.authenticated.is_some();
    let roles = &access.roles;
    let permissions = &access.permissions;
    let can = match &access.can {
        Some((can, _)) => {
            let name = |name: &Name, marker: &str, constant: &str| match name {
                Name::Literal(literal) => literal.to_token_stream(),
                Name::Type(ty) => {
                    let (marker, constant) = (
                        Ident::new(marker, ty.span()),
                        Ident::new(constant, ty.span()),
                    );
                    quote_spanned!(ty.span()=> <#ty as #private::#marker>::#constant)
                }
            };
            let action = name(&can.action, "ActionMarker", "NAME");
            let subject = name(&can.subject, "Subject", "SUBJECT");
            let from = match &can.from {
                Some(from) => quote!(::core::option::Option::Some(#from)),
                None => quote!(::core::option::Option::None),
            };
            quote! {
                ::core::option::Option::Some(#private::CanAccess {
                    action: #action,
                    subject: #subject,
                    from: #from,
                })
            }
        }
        None => quote!(::core::option::Option::None),
    };
    quote! {
        #private::FieldAccess {
            public: #public,
            authenticated: #authenticated,
            roles: &[#(#roles),*],
            permissions: &[#(#permissions),*],
            can: #can,
        }
    }
}
