//! `#[controller]`: implements `navree_http::Controller` for an impl block
//! whose methods carry route attributes.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ImplItem, ImplItemFn, Item, ItemImpl, Lit, LitStr, ReceiverKind, Token};

use crate::attr::Errors;
use crate::path;
use crate::paths::{ACCESS, AUTH, FLAGS, HTTP};
use crate::route::{Api, Can, Name, Route, RouteAttrs, Site};

/// Expands `#[controller(args)] impl X { .. }`.
///
/// Errors are reported together with the best expansion possible: the impl
/// block without the route attributes and, when the arguments parsed, the
/// `impl Controller` built from the routes that did parse, so one mistake
/// does not cascade into "unused" or "not a Controller" errors.
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
    let args = errors.keep(syn::parse2::<ControllerArgs>(args));
    if let Some(args) = &args {
        errors.keep(path::check(&args.prefix));
    }
    let shared = RouteAttrs::take(&mut item.attrs, Site::Impl, &mut errors);
    if let (Some(_), Some((restriction, at))) = (&shared.access.public, shared.access.restriction())
    {
        errors.push(at.error(format!(
            "the impl block is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
        )));
    }
    let prefix = args.as_ref().map(|args| args.prefix.value());
    let routes = collect_routes(&mut item, &shared, prefix.as_deref(), &mut errors);
    if let Some(prefix) = &prefix {
        check_duplicates(&routes, prefix, &mut errors);
    }
    let paths = errors.keep(Paths::resolve(&routes));
    let mut out = errors
        .finish()
        .err()
        .map(syn::Error::into_compile_error)
        .unwrap_or_default();
    match (args, paths) {
        (Some(args), Some(paths)) => out.extend(generate(&args, &item, &routes, &paths)),
        _ => out.extend(item.into_token_stream()),
    }
    out
}

/// Parses the annotated item, which must be an inherent impl block.
fn parse_impl(item: TokenStream) -> syn::Result<ItemImpl> {
    match syn::parse2::<Item>(item)? {
        Item::Impl(item) => match &item.trait_ {
            None => Ok(item),
            Some((path, _)) => Err(syn::Error::new_spanned(
                path,
                "#[controller] goes on an inherent impl block (`impl DealController { .. }`), \
                 not on a trait impl",
            )),
        },
        other => Err(syn::Error::new(
            other.span(),
            "#[controller] goes on an impl block: \
             `#[controller(\"/deals\")] impl DealController { .. }`",
        )),
    }
}

/// `#[get]` and friends outside a `#[controller]` impl block (inside one,
/// `#[controller]` consumes them before they expand): an error, plus the item
/// unchanged so the error does not cascade.
pub(crate) fn outside_controller(method: &str, item: TokenStream) -> TokenStream {
    let message = format!(
        "#[{method}] only works on a method of an impl block marked #[controller(\"/prefix\")]"
    );
    let mut out = syn::Error::new(Span::call_site(), message).into_compile_error();
    out.extend(item);
    out
}

/// `#[controller("/prefix", version = "1")]`.
struct ControllerArgs {
    prefix: LitStr,
    version: Option<LitStr>,
}

impl Parse for ControllerArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let prefix = if input.peek(LitStr) {
            input.parse()?
        } else {
            LitStr::new("", Span::call_site())
        };
        let mut version = None;
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            if key != "version" {
                return Err(syn::Error::new(
                    key.span(),
                    format!("unknown #[controller] key `{key}`: expected `version = \"1\"`"),
                ));
            }
            input.parse::<Token![=]>()?;
            let value = match input.parse::<Lit>()? {
                Lit::Str(s) => s,
                Lit::Int(i) => LitStr::new(i.base10_digits(), i.span()),
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "expected a version: version = \"1\"",
                    ));
                }
            };
            let text = value.value();
            let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
            if text.is_empty() || !text.chars().all(valid) {
                return Err(syn::Error::new(
                    value.span(),
                    format!(
                        "invalid API version `{text}`: expected letters, digits, `.`, `-` or \
                         `_`, e.g. version = \"1\" (mounted under /v1)"
                    ),
                ));
            }
            version = Some(value);
        }
        Ok(ControllerArgs { prefix, version })
    }
}

/// The end of the "public and restricted" error.
const PUBLIC_CONFLICT: &str =
    "a public route needs no authentication or authorization; remove one of them";

/// The end of the "two abilities" error.
const ONE_ABILITY: &str =
    "a route checks one ability; move the impl block's #[can] to the routes that need it";

/// The middle of the "no access declaration" error (closed by the caller).
const DECLARE_ACCESS: &str =
    "add #[public], #[authenticated], #[roles], #[permissions] or #[can] (deny by default";

/// The route attributes, for error messages.
const ROUTE_ATTRIBUTES: &str = "(#[get], #[post], #[put], #[patch] or #[delete])";

/// Paths to the runtime crates the expansion names.
struct Paths {
    /// navree-http, always needed.
    http: TokenStream,
    /// navree-access, needed for `#[can]` with marker types.
    access: Option<TokenStream>,
    /// navree-flags, needed for `#[requires_flag]`.
    flags: Option<TokenStream>,
    /// navree-auth, needed for `#[not_while_impersonating]` and
    /// `#[recent_auth]`.
    auth: Option<TokenStream>,
}

impl Paths {
    fn resolve(routes: &[RouteFn]) -> syn::Result<Paths> {
        let http = HTTP.path(Span::call_site())?;
        let markers = routes.iter().find_map(|route| {
            route
                .attrs
                .access
                .can
                .as_ref()
                .filter(|(can, _)| can.uses_markers())
        });
        let access = match markers {
            Some((_, at)) => Some(ACCESS.path(at.span())?),
            None => None,
        };
        let flagged = routes
            .iter()
            .find_map(|route| route.attrs.requires_flag.as_ref());
        let flags = match flagged {
            Some(requires) => Some(FLAGS.path(requires.at.span())?),
            None => None,
        };
        let auth = match routes.iter().find_map(|route| route.attrs.auth_attribute()) {
            Some(at) => Some(AUTH.path(at.span())?),
            None => None,
        };
        Ok(Paths {
            http,
            access,
            flags,
            auth,
        })
    }
}

/// One route method.
struct RouteFn {
    ident: Ident,
    route: Route,
    attrs: RouteAttrs,
    /// The extractor argument types, in order.
    args: Vec<syn::Type>,
    /// The method's `#[cfg(..)]` attributes, which gate the route too.
    cfgs: Vec<syn::Attribute>,
}

/// Strips the route attributes from the methods of `item` and collects the
/// routes, each combined with the `shared` attributes of the impl block.
/// `prefix` is the controller prefix, when it parsed.
fn collect_routes(
    item: &mut ItemImpl,
    shared: &RouteAttrs,
    prefix: Option<&str>,
    errors: &mut Errors,
) -> Vec<RouteFn> {
    let mut routes = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let ident = method.sig.ident.clone();
        let mut attrs = RouteAttrs::take(&mut method.attrs, Site::Method(&ident), errors);
        let Some(route) = attrs.route.take() else {
            if let (Some((name, at)), false) = (&attrs.first_helper, attrs.malformed_route) {
                errors.push(at.error(format!(
                    "#[{name}] only applies to routes: `{ident}` has no route attribute \
                     {ROUTE_ATTRIBUTES}"
                )));
            }
            continue;
        };
        check_route(&ident, &route, &mut attrs, shared, prefix, errors);
        // A route whose signature is wrong is reported and left out, so the
        // generated call does not add confusing type errors.
        if errors.keep(check_signature(method)).is_none() {
            continue;
        }
        routes.push(RouteFn {
            ident,
            route,
            attrs,
            args: extractor_types(method),
            cfgs: method
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("cfg"))
                .cloned()
                .collect(),
        });
    }
    routes
}

/// Checks one route and adds the impl block's `shared` attributes to it.
fn check_route(
    ident: &Ident,
    route: &Route,
    attrs: &mut RouteAttrs,
    shared: &RouteAttrs,
    prefix: Option<&str>,
    errors: &mut Errors,
) {
    errors.keep(path::check(&route.path));
    if let Some((restriction, at)) = attrs.access.public_conflict(&shared.access) {
        errors.push(at.error(format!(
            "route `{ident}` is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
        )));
    }
    if let (Some((_, at)), Some(_)) = (&attrs.access.can, &shared.access.can) {
        errors.push(at.error(format!(
            "route `{ident}` has #[can] here and on the impl block: {ONE_ABILITY}"
        )));
    }
    attrs.inherit(shared);
    if let (Some(prefix), Some((can, _))) = (prefix, &attrs.access.can) {
        errors.keep(check_from(can, prefix, route));
    }
    if !attrs.access.is_declared() && !attrs.access.malformed {
        let guard_note = if attrs.guards.is_empty() {
            ""
        } else {
            "; a #[guard] alone is not an access declaration"
        };
        errors.push(syn::Error::new(
            ident.span(),
            format!("route `{ident}` has no access declaration: {DECLARE_ACCESS}{guard_note})"),
        ));
    }
}

/// Two routes with the same method and path (capture names aside) would
/// collide in the router. Routes behind `#[cfg]` are left to navree-http's
/// boot check: their conditions may exclude each other.
fn check_duplicates(routes: &[RouteFn], prefix: &str, errors: &mut Errors) {
    let template = |route: &RouteFn| path::route_template(prefix, &route.route.path.value());
    let mut seen: Vec<(&str, String, &RouteFn)> = Vec::new();
    for route in routes.iter().filter(|route| route.cfgs.is_empty()) {
        let method = route.route.method.as_str();
        let shape = path::shape(&template(route));
        match seen.iter().find(|(m, s, _)| (*m, s) == (method, &shape)) {
            Some((_, _, first)) => errors.push(syn::Error::new(
                route.route.path.span(),
                format!(
                    "route `{}` has the same method and path as route `{}`: {method} {}",
                    route.ident,
                    first.ident,
                    template(first),
                ),
            )),
            None => seen.push((method, shape, route)),
        }
    }
}

/// A route handler is a non-generic `async fn` taking `&self`.
fn check_signature(method: &ImplItemFn) -> syn::Result<()> {
    let sig = &method.sig;
    let ident = &sig.ident;
    let mut errors = Errors::default();
    if sig.asyncness.is_none() {
        errors.push(syn::Error::new(
            ident.span(),
            format!("route handler `{ident}` must be an `async fn`"),
        ));
    }
    let receiver_error = format!(
        "route handler `{ident}` must take `&self`: the generated handler calls it on the shared \
         controller"
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
            format!("route handler `{ident}` cannot be generic: extractors need concrete types"),
        ));
    }
    errors.finish()
}

/// `from = "id"` must name a parameter of the route's path (the record id
/// comes from it).
fn check_from(can: &Can, prefix: &str, route: &Route) -> syn::Result<()> {
    let Some(from) = &can.from else {
        return Ok(());
    };
    let template = path::route_template(prefix, &route.path.value());
    let name = from.value();
    if path::params(&template).contains(&name.as_str()) {
        return Ok(());
    }
    Err(syn::Error::new(
        from.span(),
        format!(
            "`from = \"{name}\"` names no parameter of the route path `{template}`: the record \
             id must come from a path parameter such as `{{{name}}}`"
        ),
    ))
}

/// The types of the method's arguments after `&self`.
fn extractor_types(method: &ImplItemFn) -> Vec<syn::Type> {
    method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(arg) => Some((*arg.ty).clone()),
            FnArg::Receiver(_) => None,
        })
        .collect()
}

/// The impl block plus its `impl Controller`.
fn generate(
    args: &ControllerArgs,
    item: &ItemImpl,
    routes: &[RouteFn],
    paths: &Paths,
) -> TokenStream {
    let http = &paths.http;
    let prefix = &args.prefix;
    let version = match &args.version {
        Some(version) => quote!(::core::option::Option::Some(#version)),
        None => quote!(::core::option::Option::None),
    };
    // Mixed-site hygiene: expressions written by the user (guards) can
    // neither see nor shadow the generated locals.
    let list = Ident::new("routes", Span::mixed_site());
    let self_ty = &item.self_ty;
    let type_name = handler_type_name(self_ty);
    let routes = routes.iter().map(|route| {
        let cfgs = &route.cfgs;
        let def = route_def(route, paths);
        let handler = LitStr::new(&format!("{type_name}::{}", route.ident), route.ident.span());
        quote!(#(#cfgs)* #list.push(#def.handler_name(#handler));)
    });

    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    quote! {
        #item

        impl #impl_generics #http::Controller for #self_ty #where_clause {
            fn prefix(&self) -> &'static str {
                #prefix
            }

            fn version(&self) -> ::core::option::Option<&'static str> {
                #version
            }

            // `ApiMeta { .., ..Default::default() }` keeps compiling when
            // navree-http adds fields, even if `#[api]` sets all of today's.
            #[allow(clippy::needless_update)]
            fn routes(self: ::std::sync::Arc<Self>) -> ::std::vec::Vec<#http::RouteDef> {
                let mut #list = ::std::vec::Vec::new();
                #(#routes)*
                #list
            }
        }
    }
}

/// The controller's type as route listings show it: the last path segment
/// without generics (`crate::deals::DealsController<T>` → `DealsController`).
fn handler_type_name(self_ty: &syn::Type) -> String {
    match self_ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default(),
        other => quote!(#other).to_string().replace(' ', ""),
    }
}

/// The `RouteDef` expression of one route. Guard expressions see `self`
/// (the `Arc<Self>` of `routes`).
fn route_def(route: &RouteFn, paths: &Paths) -> TokenStream {
    let http = &paths.http;
    let local = |name: &str| Ident::new(name, Span::mixed_site());
    let (this, method_var, guard_var) = (local("this"), local("method"), local("guard"));
    let ident = &route.ident;
    let path = &route.route.path;
    let method = &route.route.method;
    let args: Vec<Ident> = (0..route.args.len())
        .map(|i| local(&format!("arg{i}")))
        .collect();
    let params = args
        .iter()
        .zip(&route.args)
        .map(|(arg, ty)| quote_spanned!(ty.span()=> #arg: #ty));
    let access = access_calls(&route.attrs, paths.access.as_ref());
    let guards = route.attrs.guards.iter().map(|guard| {
        quote_spanned! {guard.span()=>
            .guard({
                let #guard_var: ::std::sync::Arc<dyn #http::Guard> = #guard;
                #guard_var
            })
        }
    });
    let auth_guards = paths.auth.as_ref().map(|auth| {
        let impersonation = route
            .attrs
            .not_while_impersonating
            .as_ref()
            .map(|at| quote_spanned!(at.span()=> .guard(#auth::not_while_impersonating())));
        let recent = route.attrs.recent_auth.as_ref().map(|(secs, at)| {
            quote_spanned! {at.span()=>
                .guard(#auth::recent_auth(::core::time::Duration::from_secs(#secs)))
            }
        });
        quote!(#impersonation #recent)
    });
    let options = options(&route.attrs);
    let api = route.attrs.api.as_ref().map(|api| api_call(api, http));
    let mut def = quote! {
        #http::RouteDef::on(#method_var, #path, move |#(#params),*| async move {
            Self::#ident(&*#this, #(#args),*).await
        })
        #access
    };
    // The flag's guard goes first, so the route "does not exist" before
    // any other guard runs.
    if let (Some(requires), Some(flags)) = (&route.attrs.requires_flag, &paths.flags) {
        let (flag, status) = (&requires.flag, requires.status);
        def = quote_spanned! {flag.span()=>
            #flags::requires_flag::<#flag>(#def, #status)
        };
    }
    quote! {
        {
            // `http::Method` without naming the `http` crate: inferred from
            // `RouteDef::on`'s parameter.
            let #method_var = match ::core::str::FromStr::from_str(#method) {
                ::core::result::Result::Ok(#method_var) => #method_var,
                ::core::result::Result::Err(_) => ::core::unreachable!(),
            };
            let #this = ::std::sync::Arc::clone(&self);
            #def
            #auth_guards
            #(#guards)*
            #options
            #api
        }
    }
}

/// The throttle, timeout, transactional, audit and name builder calls.
fn options(attrs: &RouteAttrs) -> TokenStream {
    let duration = |millis: u64| quote!(::core::time::Duration::from_millis(#millis));
    let mut out = TokenStream::new();
    if let Some(throttle) = attrs.throttle {
        let limit = throttle.limit;
        let per = duration(throttle.per_millis);
        out.extend(quote!(.throttle(#limit, #per)));
    }
    if let Some(millis) = attrs.timeout_millis {
        let timeout = duration(millis);
        out.extend(quote!(.timeout(#timeout)));
    }
    if attrs.transactional {
        out.extend(quote!(.transactional()));
    }
    if attrs.no_compression {
        out.extend(quote!(.no_compression()));
    }
    match attrs.page {
        Some(true) => out.extend(quote!(.page())),
        Some(false) => out.extend(quote!(.json_only())),
        None => {}
    }
    if let Some(action) = &attrs.audit {
        out.extend(quote!(.audit(#action)));
    }
    if let Some(name) = &attrs.name {
        out.extend(quote!(.name(#name)));
    }
    out
}

/// `.api(ApiMeta { .. })` with the fields `#[api]` sets.
fn api_call(api: &Api, http: &TokenStream) -> TokenStream {
    let string = |s: &LitStr| quote!(::std::string::String::from(#s));
    let mut fields = TokenStream::new();
    if let Some(summary) = &api.summary {
        let summary = string(summary);
        fields.extend(quote!(summary: ::core::option::Option::Some(#summary),));
    }
    if let Some(description) = &api.description {
        let description = string(description);
        fields.extend(quote!(description: ::core::option::Option::Some(#description),));
    }
    if !api.tags.is_empty() {
        let tags = api.tags.iter().map(string);
        fields.extend(quote!(tags: ::std::vec![#(#tags),*],));
    }
    if !api.responses.is_empty() {
        let responses = &api.responses;
        fields.extend(quote!(responses: ::std::vec![#(#responses),*],));
    }
    if let Some(status) = api.status {
        fields.extend(quote!(success_status: #status,));
    }
    let schema = |ty: &syn::Type| {
        quote_spanned!(ty.span()=>
            ::core::option::Option::Some(#http::schema_fn::<#ty> as #http::SchemaFn)
        )
    };
    if let Some(request) = &api.request {
        let request = schema(request);
        fields.extend(quote!(request: #request,));
    }
    if let Some(response) = &api.response {
        let response = schema(response);
        fields.extend(quote!(response: #response,));
    }
    if let Some(query) = &api.query {
        let query = quote_spanned!(query.span()=>
            ::core::option::Option::Some(#http::list_spec::<#query> as #http::ListSpecFn)
        );
        fields.extend(quote!(query: #query,));
    }
    quote! {
        .api(#http::ApiMeta {
            #fields
            ..::core::default::Default::default()
        })
    }
}

/// The access builder calls of a route (or a gRPC method, whose builder
/// has the same methods). `access_crate` is the navree-access path,
/// resolved whenever a `#[can]` names marker types.
pub(crate) fn access_calls(attrs: &RouteAttrs, access_crate: Option<&TokenStream>) -> TokenStream {
    let access = &attrs.access;
    let mut out = TokenStream::new();
    if access.public.is_some() {
        out.extend(quote!(.public()));
    }
    if access.authenticated.is_some() {
        out.extend(quote!(.authenticated()));
    }
    if !access.roles.is_empty() {
        let roles = &access.roles;
        out.extend(quote!(.roles(&[#(#roles),*])));
    }
    if !access.permissions.is_empty() {
        let permissions = &access.permissions;
        out.extend(quote!(.permissions(&[#(#permissions),*])));
    }
    if let Some((can, _)) = &access.can {
        out.extend(can_call(can, access_crate));
    }
    out
}

/// `.can(action, subject, from)`: marker types become their constants
/// (`<Update as ActionMarker>::NAME`, `<Deal as Subject>::SUBJECT`).
pub(crate) fn can_call(can: &Can, access_crate: Option<&TokenStream>) -> TokenStream {
    let name = |name: &Name, marker: &str, constant: &str| match name {
        Name::Literal(literal) => literal.to_token_stream(),
        Name::Type(ty) => {
            let (marker, constant) = (
                Ident::new(marker, ty.span()),
                Ident::new(constant, ty.span()),
            );
            quote_spanned!(ty.span()=> <#ty as #access_crate::#marker>::#constant)
        }
    };
    let action = name(&can.action, "ActionMarker", "NAME");
    let subject = name(&can.subject, "Subject", "SUBJECT");
    let from = match &can.from {
        Some(from) => quote!(::core::option::Option::Some(#from)),
        None => quote!(::core::option::Option::None),
    };
    quote!(.can(#action, #subject, #from))
}
