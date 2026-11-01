//! `#[message_handlers]`: implements `navree_micro::MessageHandlers` for an
//! impl block whose methods carry `#[message_pattern]` and
//! `#[event_pattern]`.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, FnArg, Ident, ImplItem, ImplItemFn, Item, ItemImpl, LitInt, LitStr, Meta,
    ReceiverKind, Token,
};

use crate::attr::Errors;
use crate::controller::can_call;
use crate::paths::{ACCESS, HTTP, MICRO};
use crate::route::{self, Access, At, Can};

/// The attributes `#[message_handlers]` understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    MessagePattern,
    EventPattern,
    Public,
    Authenticated,
    Roles,
    Permissions,
    Can,
    Guard,
    Timeout,
}

/// Route attributes that do not apply to message handlers.
const ROUTE_ONLY: [&str; 11] = [
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "throttle",
    "transactional",
    "no_compression",
    "audit",
    "name",
    "api",
];

impl Kind {
    /// The attribute `attr` is, when it is ours (plain, or qualified by
    /// `navree::` / `navree_macros::`); `Err` with the name of a route-only
    /// attribute.
    fn of(attr: &Attribute) -> Option<Result<Kind, String>> {
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
        let name = path.segments.last()?.ident.to_string();
        Some(Ok(match name.as_str() {
            "message_pattern" => Kind::MessagePattern,
            "event_pattern" => Kind::EventPattern,
            "public" => Kind::Public,
            "authenticated" => Kind::Authenticated,
            "roles" => Kind::Roles,
            "permissions" => Kind::Permissions,
            "can" => Kind::Can,
            "guard" => Kind::Guard,
            "timeout" => Kind::Timeout,
            other if ROUTE_ONLY.contains(&other) => return Some(Err(other.to_owned())),
            _ => return None,
        }))
    }

    fn is_pattern(self) -> bool {
        matches!(self, Kind::MessagePattern | Kind::EventPattern)
    }

    fn is_access(self) -> bool {
        matches!(
            self,
            Kind::Public | Kind::Authenticated | Kind::Roles | Kind::Permissions | Kind::Can
        )
    }

    fn allowed_on_impl(self) -> bool {
        self.is_access() || self == Kind::Guard
    }

    fn repeatable(self) -> bool {
        matches!(self, Kind::Roles | Kind::Permissions | Kind::Guard)
    }
}

/// `#[message_pattern("deals.get", dedupe, concurrency = 2)]` or
/// `#[event_pattern("deal.won", max_attempts = 10, ordered, broadcast, concurrency = 4)]`.
struct PatternAttr {
    kind: Kind,
    pattern: LitStr,
    dedupe: bool,
    concurrency: Option<LitInt>,
    max_attempts: Option<LitInt>,
    ordered: bool,
    broadcast: bool,
}

impl PatternAttr {
    fn parse(kind: Kind, attr: &Attribute) -> syn::Result<Self> {
        let at = At::of(attr);
        let (name, keys) = match kind {
            Kind::MessagePattern => ("message_pattern", "`dedupe` or `concurrency = n`"),
            _ => (
                "event_pattern",
                "`max_attempts = n`, `ordered`, `broadcast` or `concurrency = n`",
            ),
        };
        let expected = || at.error(format!("expected a pattern: #[{name}(\"deals.get\")]"));
        if !matches!(attr.meta, Meta::List(_)) {
            return Err(expected());
        }
        attr.parse_args_with(|input: ParseStream| {
            let pattern: LitStr = input.parse().map_err(|_| expected())?;
            let mut parsed = PatternAttr {
                kind,
                pattern,
                dedupe: false,
                concurrency: None,
                max_attempts: None,
                ordered: false,
                broadcast: false,
            };
            let mut seen: Vec<String> = Vec::new();
            while input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
                let key: Ident = input.parse()?;
                let key_name = key.to_string();
                if seen.contains(&key_name) {
                    return Err(syn::Error::new(key.span(), format!("duplicate `{key_name}`")));
                }
                seen.push(key_name.clone());
                match (kind, key_name.as_str()) {
                    (Kind::MessagePattern, "dedupe") => parsed.dedupe = true,
                    (Kind::EventPattern, "ordered") => parsed.ordered = true,
                    (Kind::EventPattern, "broadcast") => parsed.broadcast = true,
                    (_, "concurrency") => {
                        input.parse::<Token![=]>()?;
                        parsed.concurrency = Some(positive(input.parse()?, "concurrency")?);
                    }
                    (Kind::EventPattern, "max_attempts") => {
                        input.parse::<Token![=]>()?;
                        parsed.max_attempts = Some(positive(input.parse()?, "max_attempts")?);
                    }
                    (Kind::MessagePattern, "max_attempts" | "ordered" | "broadcast") => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "`{key_name}` applies to #[event_pattern]: requests are not retried by the broker"
                            ),
                        ));
                    }
                    (Kind::EventPattern, "dedupe") => {
                        return Err(syn::Error::new(
                            key.span(),
                            "`dedupe` applies to #[message_pattern]: the inbox already deduplicates events",
                        ));
                    }
                    _ => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown #[{name}] key `{key_name}`: expected {keys}"),
                        ));
                    }
                }
            }
            if !input.is_empty() {
                return Err(input.error(format!("unexpected tokens: expected {keys}")));
            }
            if let Err(reason) = check_pattern(&parsed.pattern.value(), kind == Kind::EventPattern) {
                return Err(syn::Error::new(
                    parsed.pattern.span(),
                    format!("invalid pattern `{}`: {reason}", parsed.pattern.value()),
                ));
            }
            if parsed.broadcast && parsed.ordered {
                return Err(at.error("`broadcast` and `ordered` exclude each other"));
            }
            Ok(parsed)
        })
    }
}

/// A whole number of at least 1.
fn positive(value: LitInt, key: &str) -> syn::Result<LitInt> {
    match value.base10_parse::<u32>() {
        Ok(n) if n > 0 => Ok(value),
        _ => Err(syn::Error::new(
            value.span(),
            format!("`{key}` must be a whole number of at least 1"),
        )),
    }
}

/// The pattern syntax of navree-micro's `check_pattern`.
fn check_pattern(pattern: &str, wildcards: bool) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("a pattern cannot be empty".to_owned());
    }
    let segments: Vec<&str> = pattern.split('.').collect();
    let last = segments.len() - 1;
    for (i, segment) in segments.iter().enumerate() {
        match *segment {
            "" => return Err("segments are separated by single dots".to_owned()),
            "*" | ">" if !wildcards => {
                return Err(
                    "a message pattern names exactly one handler, so it has no wildcards"
                        .to_owned(),
                );
            }
            "*" => {}
            ">" if i == last => {}
            ">" => return Err("`>` may only be the last segment".to_owned()),
            segment => {
                if !segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
                {
                    return Err(format!(
                        "segment `{segment}` may only hold lower-case letters, digits, `_` and `-`"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// What the attributes of one method (or of the impl block) declare.
#[derive(Default)]
struct Attrs {
    pattern: Option<(PatternAttr, At)>,
    access: Access,
    /// Method-level access attributes, with where they were written (for
    /// the "events take no access" error).
    access_written: Vec<(String, At)>,
    guards: Vec<Expr>,
    timeout_millis: Option<u64>,
    /// The first attribute, for "only applies to patterns" errors.
    first: Option<(String, At)>,
    /// A pattern attribute was written but did not parse.
    malformed_pattern: bool,
}

/// Where the attributes were written.
#[derive(Clone, Copy)]
enum Site {
    Impl,
    Method,
}

impl Attrs {
    /// Removes our attributes from `attrs` and parses them.
    fn take(attrs: &mut Vec<Attribute>, site: Site, errors: &mut Errors) -> Self {
        let mut parsed = Attrs::default();
        let mut seen: Vec<Kind> = Vec::new();
        attrs.retain(|attr| {
            let Some(kind) = Kind::of(attr) else {
                return true;
            };
            let at = At::of(attr);
            let name = route::short_name(attr);
            let kind = match kind {
                Ok(kind) => kind,
                Err(route_only) => {
                    errors.push(at.error(format!(
                        "#[{route_only}] does not apply to message handlers (it is a #[controller] route attribute)"
                    )));
                    return false;
                }
            };
            if let Site::Impl = site {
                if kind.is_pattern() {
                    errors.push(at.error(format!(
                        "#[{name}] goes on a method of the impl block, not on the impl block"
                    )));
                    return false;
                }
                if !kind.allowed_on_impl() {
                    errors.push(at.error(format!(
                        "#[{name}] goes on a handler method, not on the impl block: on the impl block only #[public], #[authenticated], #[roles], #[permissions], #[can] and #[guard] apply"
                    )));
                    return false;
                }
            }
            if !kind.repeatable() {
                let clash = seen
                    .iter()
                    .any(|k| *k == kind || (k.is_pattern() && kind.is_pattern()));
                if clash {
                    let message = if kind.is_pattern() {
                        "a method answers one pattern: it already has a #[message_pattern] or #[event_pattern]".to_owned()
                    } else {
                        format!("duplicate #[{name}]")
                    };
                    errors.push(at.error(message));
                    return false;
                }
            }
            seen.push(kind);
            if !kind.is_pattern() && parsed.first.is_none() {
                parsed.first = Some((name.clone(), at.clone()));
            }
            if kind.is_access() {
                parsed.access_written.push((name.clone(), at.clone()));
            }
            if let Err(error) = parsed.parse(kind, attr, &at) {
                match kind {
                    Kind::MessagePattern | Kind::EventPattern => parsed.malformed_pattern = true,
                    kind if kind.is_access() => parsed.access.malformed = true,
                    _ => {}
                }
                errors.push(error);
            }
            false
        });
        parsed
    }

    fn parse(&mut self, kind: Kind, attr: &Attribute, at: &At) -> syn::Result<()> {
        match kind {
            Kind::MessagePattern | Kind::EventPattern => {
                self.pattern = Some((PatternAttr::parse(kind, attr)?, at.clone()));
            }
            Kind::Public => self.access.public = Some(route::flag(attr)?),
            Kind::Authenticated => self.access.authenticated = Some(route::flag(attr)?),
            Kind::Roles => {
                let roles = route::strings(attr, "\"admin\", \"manager\"")?;
                self.access.roles.extend(roles);
                self.access.roles_at.get_or_insert(at.clone());
            }
            Kind::Permissions => {
                let permissions = route::strings(attr, "\"deals.update\"")?;
                self.access.permissions.extend(permissions);
                self.access.permissions_at.get_or_insert(at.clone());
            }
            Kind::Can => {
                let can = Can::parse_args(attr, at)?;
                self.access.can = Some((can, at.clone()));
            }
            Kind::Guard => {
                let expected = || at.error("expected a guard: #[guard(self.internal.clone())]");
                let guards = match &attr.meta {
                    Meta::List(_) => {
                        attr.parse_args_with(Punctuated::<Expr, Token![,]>::parse_terminated)?
                    }
                    _ => return Err(expected()),
                };
                if guards.is_empty() {
                    return Err(expected());
                }
                self.guards.extend(guards);
            }
            Kind::Timeout => {
                let value: LitStr = attr
                    .parse_args()
                    .map_err(|_| at.error("expected a duration: #[timeout(\"5s\")]"))?;
                self.timeout_millis = Some(route::duration_millis(&value)?);
            }
        }
        Ok(())
    }
}

/// One handler method.
struct HandlerFn {
    ident: Ident,
    pattern: PatternAttr,
    access: Access,
    guards: Vec<Expr>,
    timeout_millis: Option<u64>,
    args: Vec<syn::Type>,
    cfgs: Vec<Attribute>,
}

/// Expands `#[message_handlers] impl X { .. }`.
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
            "#[message_handlers] takes no arguments: the patterns are on the methods",
        ));
    }
    let shared = Attrs::take(&mut item.attrs, Site::Impl, &mut errors);
    if let (Some(_), Some((restriction, at))) = (&shared.access.public, shared.access.restriction())
    {
        errors.push(at.error(format!(
            "the impl block is #[public] but also declares {restriction}: a public pattern needs no authentication or authorization; remove one of them"
        )));
    }
    let handlers = collect(&mut item, &shared, &mut errors);
    check_duplicates(&handlers, &mut errors);
    let paths = errors.keep(Paths::resolve(&handlers));
    let mut out = errors
        .finish()
        .err()
        .map(syn::Error::into_compile_error)
        .unwrap_or_default();
    match paths {
        Some(paths) => out.extend(generate(&item, &handlers, &paths)),
        None => out.extend(item.into_token_stream()),
    }
    out
}

/// `#[message_pattern]` or `#[event_pattern]` outside `#[message_handlers]`.
pub(crate) fn outside(name: &str, item: TokenStream) -> TokenStream {
    let message =
        format!("#[{name}] only works on a method of an impl block marked #[message_handlers]");
    let mut out = syn::Error::new(Span::call_site(), message).into_compile_error();
    out.extend(item);
    out
}

fn parse_impl(item: TokenStream) -> syn::Result<ItemImpl> {
    match syn::parse2::<Item>(item)? {
        Item::Impl(item) => match &item.trait_ {
            None => Ok(item),
            Some((path, _)) => Err(syn::Error::new_spanned(
                path,
                "#[message_handlers] goes on an inherent impl block (`impl DealsHandlers { .. }`), not on a trait impl",
            )),
        },
        other => Err(syn::Error::new(
            other.span(),
            "#[message_handlers] goes on an impl block: `#[message_handlers] impl DealsHandlers { .. }`",
        )),
    }
}

/// Paths to the runtime crates the expansion names.
struct Paths {
    micro: TokenStream,
    http: Option<TokenStream>,
    access: Option<TokenStream>,
}

impl Paths {
    fn resolve(handlers: &[HandlerFn]) -> syn::Result<Paths> {
        let micro = MICRO.path(Span::call_site())?;
        let http = match handlers.iter().find_map(|h| h.guards.first()) {
            Some(guard) => Some(HTTP.path(guard.span())?),
            None => None,
        };
        let markers = handlers
            .iter()
            .find_map(|h| h.access.can.as_ref().filter(|(can, _)| can.uses_markers()));
        let access = match markers {
            Some((_, at)) => Some(ACCESS.path(at.span())?),
            None => None,
        };
        Ok(Paths {
            micro,
            http,
            access,
        })
    }
}

fn collect(item: &mut ItemImpl, shared: &Attrs, errors: &mut Errors) -> Vec<HandlerFn> {
    let mut handlers = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let ident = method.sig.ident.clone();
        let mut attrs = Attrs::take(&mut method.attrs, Site::Method, errors);
        let Some((pattern, _)) = attrs.pattern.take() else {
            if let (Some((name, at)), false) = (&attrs.first, attrs.malformed_pattern) {
                errors.push(at.error(format!(
                    "#[{name}] only applies to handlers: `{ident}` has no #[message_pattern] or #[event_pattern]"
                )));
            }
            continue;
        };
        let value = pattern.pattern.value();
        let mut access = Access::default();
        match pattern.kind {
            Kind::EventPattern => {
                if let Some((name, at)) = attrs.access_written.first() {
                    errors.push(at.error(format!(
                        "event pattern `{value}` (`{ident}`) takes no access declaration: events are facts, and the access check ran where the change was made; remove #[{name}]"
                    )));
                }
            }
            _ => {
                access = std::mem::take(&mut attrs.access);
                if let Some((restriction, at)) = access.public_conflict(&shared.access) {
                    errors.push(at.error(format!(
                        "message pattern `{value}` (`{ident}`) is #[public] but also declares {restriction}: a public pattern needs no authentication or authorization; remove one of them"
                    )));
                }
                if let (Some((_, at)), Some(_)) = (&access.can, &shared.access.can) {
                    errors.push(at.error(format!(
                        "message pattern `{value}` (`{ident}`) has #[can] here and on the impl block: a pattern checks one ability"
                    )));
                }
                inherit(&mut access, &shared.access);
                if !access.is_declared() && !access.malformed {
                    let guard_note = if attrs.guards.is_empty() && shared.guards.is_empty() {
                        ""
                    } else {
                        "; a #[guard] alone is not an access declaration"
                    };
                    errors.push(syn::Error::new(
                        ident.span(),
                        format!(
                            "message pattern `{value}` (`{ident}`) has no access declaration: add #[public], #[authenticated], #[roles], #[permissions] or #[can] (deny by default{guard_note})"
                        ),
                    ));
                }
            }
        }
        if errors.keep(check_signature(method)).is_none() {
            continue;
        }
        let mut guards = shared.guards.clone();
        guards.extend(attrs.guards);
        handlers.push(HandlerFn {
            ident,
            pattern,
            access,
            guards,
            timeout_millis: attrs.timeout_millis,
            args: arg_types(method),
            cfgs: method
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("cfg"))
                .cloned()
                .collect(),
        });
    }
    handlers
}

/// The impl block's access declarations add to a message pattern's own.
fn inherit(access: &mut Access, shared: &Access) {
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
    access.malformed |= shared.malformed;
}

/// Two handlers for the same pattern of the same kind.
fn check_duplicates(handlers: &[HandlerFn], errors: &mut Errors) {
    let mut seen: Vec<(Kind, String, &Ident)> = Vec::new();
    for handler in handlers.iter().filter(|h| h.cfgs.is_empty()) {
        let value = handler.pattern.pattern.value();
        match seen
            .iter()
            .find(|(kind, pattern, _)| *kind == handler.pattern.kind && *pattern == value)
        {
            Some((_, _, first)) => errors.push(syn::Error::new(
                handler.pattern.pattern.span(),
                format!(
                    "`{}` answers `{value}` too: one handler per pattern (first: `{first}`)",
                    handler.ident
                ),
            )),
            None => seen.push((handler.pattern.kind, value, &handler.ident)),
        }
    }
}

/// A handler is a non-generic `async fn` taking `&self`.
fn check_signature(method: &ImplItemFn) -> syn::Result<()> {
    let sig = &method.sig;
    let ident = &sig.ident;
    let mut errors = Errors::default();
    if sig.asyncness.is_none() {
        errors.push(syn::Error::new(
            ident.span(),
            format!("message handler `{ident}` must be an `async fn`"),
        ));
    }
    let receiver_error = format!(
        "message handler `{ident}` must take `&self`: the generated handler calls it on the shared struct"
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
            format!("message handler `{ident}` cannot be generic: extractors need concrete types"),
        ));
    }
    errors.finish()
}

fn arg_types(method: &ImplItemFn) -> Vec<syn::Type> {
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

fn generate(item: &ItemImpl, handlers: &[HandlerFn], paths: &Paths) -> TokenStream {
    let micro = &paths.micro;
    let list = Ident::new("patterns", Span::mixed_site());
    let defs = handlers.iter().map(|handler| {
        let cfgs = &handler.cfgs;
        let def = pattern_def(handler, paths);
        quote!(#(#cfgs)* #list.push(#def);)
    });
    let self_ty = &item.self_ty;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    quote! {
        #item

        impl #impl_generics #micro::MessageHandlers for #self_ty #where_clause {
            fn patterns(self: ::std::sync::Arc<Self>) -> ::std::vec::Vec<#micro::PatternDef> {
                let mut #list = ::std::vec::Vec::new();
                #(#defs)*
                #list
            }
        }
    }
}

fn pattern_def(handler: &HandlerFn, paths: &Paths) -> TokenStream {
    let micro = &paths.micro;
    let local = |name: &str| Ident::new(name, Span::mixed_site());
    let (this, codec, guard_var) = (local("this"), local("codec"), local("guard"));
    let ident = &handler.ident;
    let name = ident.to_string();
    let pattern = &handler.pattern.pattern;
    let constructor = match handler.pattern.kind {
        Kind::MessagePattern => quote!(message),
        _ => quote!(event),
    };
    let args: Vec<Ident> = (0..handler.args.len())
        .map(|i| local(&format!("arg{i}")))
        .collect();
    let params = args
        .iter()
        .zip(&handler.args)
        .map(|(arg, ty)| quote_spanned!(ty.span()=> #arg: #ty));
    let access = access_calls(&handler.access, paths.access.as_ref());
    let guards = handler.guards.iter().map(|guard| {
        let http = paths.http.as_ref();
        quote_spanned! {guard.span()=>
            .guard({
                let #guard_var: ::std::sync::Arc<dyn #http::Guard> = #guard;
                #guard_var
            })
        }
    });
    let mut options = TokenStream::new();
    if let Some(millis) = handler.timeout_millis {
        options.extend(quote!(.timeout(::core::time::Duration::from_millis(#millis))));
    }
    if handler.pattern.dedupe {
        options.extend(quote!(.dedupe()));
    }
    if let Some(limit) = &handler.pattern.concurrency {
        options.extend(quote!(.concurrency(#limit)));
    }
    if let Some(attempts) = &handler.pattern.max_attempts {
        options.extend(quote!(.max_attempts(#attempts)));
    }
    if handler.pattern.ordered {
        options.extend(quote!(.ordered()));
    }
    if handler.pattern.broadcast {
        options.extend(quote!(.broadcast()));
    }
    quote! {
        {
            let #this = ::std::sync::Arc::clone(&self);
            #micro::PatternDef::#constructor(
                #pattern,
                move |#codec: #micro::ReplyCodec, #(#params),*| async move {
                    #micro::MessageReply::of(#codec, Self::#ident(&*#this, #(#args),*).await)
                },
            )
            .handler_name(#name)
            #access
            #(#guards)*
            #options
        }
    }
}

fn access_calls(access: &Access, access_crate: Option<&TokenStream>) -> TokenStream {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_follow_the_runtime_rules() {
        assert!(check_pattern("deals.get", false).is_ok());
        assert!(check_pattern("deal.*", true).is_ok());
        assert!(check_pattern("deal.>", true).is_ok());
        for (bad, wildcards) in [
            ("", true),
            ("Deals.get", false),
            ("deal.*", false),
            ("deal..won", true),
            ("deal.>.x", true),
        ] {
            assert!(check_pattern(bad, wildcards).is_err(), "{bad}");
        }
    }
}
