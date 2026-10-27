//! The route attributes `#[controller]` reads from its methods and from the
//! impl block: the route itself (`#[get("/p")]`, ...) and the helpers
//! (`#[public]`, `#[roles(..)]`, ...).
//!
//! The helpers are inert: they are not macros of their own; `#[controller]`
//! consumes them and removes them from the expanded code.

use std::fmt::Display;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::meta::ParseNestedMeta;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, LitInt, LitStr, Meta, Token, Type};

use crate::attr::{Errors, key_name};
use crate::duration;

/// The HTTP method attributes.
const METHODS: [&str; 5] = ["get", "post", "put", "patch", "delete"];

/// One of the attributes `#[controller]` understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Route(&'static str),
    Public,
    Authenticated,
    Roles,
    Permissions,
    Can,
    Guard,
    Throttle,
    Timeout,
    Transactional,
    Audit,
    Name,
    Api,
    RequiresFlag,
}

impl Kind {
    /// The attribute `attr` is, when it is one of ours: a plain name
    /// (`#[get]`) or one qualified by the facade or this crate
    /// (`#[navree::get]`, `#[navree_macros::get]`).
    fn of(attr: &Attribute) -> Option<Kind> {
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
        if let Some(method) = METHODS.iter().find(|m| **m == name) {
            return Some(Kind::Route(method));
        }
        Some(match name.as_str() {
            "public" => Kind::Public,
            "authenticated" => Kind::Authenticated,
            "roles" => Kind::Roles,
            "permissions" => Kind::Permissions,
            "can" => Kind::Can,
            "guard" => Kind::Guard,
            "throttle" => Kind::Throttle,
            "timeout" => Kind::Timeout,
            "transactional" => Kind::Transactional,
            "audit" => Kind::Audit,
            "name" => Kind::Name,
            "api" => Kind::Api,
            "requires_flag" => Kind::RequiresFlag,
            _ => return None,
        })
    }

    /// Whether the attribute may sit on the impl block (and then applies to
    /// every route).
    fn allowed_on_impl(self) -> bool {
        matches!(
            self,
            Kind::Public
                | Kind::Authenticated
                | Kind::Roles
                | Kind::Permissions
                | Kind::Can
                | Kind::Guard
                | Kind::Throttle
        )
    }

    /// Whether the attribute declares who may call the route.
    fn is_access(self) -> bool {
        matches!(
            self,
            Kind::Public | Kind::Authenticated | Kind::Roles | Kind::Permissions | Kind::Can
        )
    }

    /// Whether a second one adds to the first (`#[roles]`, `#[permissions]`,
    /// `#[guard]`) instead of being a mistake.
    fn repeatable(self) -> bool {
        matches!(self, Kind::Roles | Kind::Permissions | Kind::Guard)
    }

    /// Whether `self` and `other` fill the same slot (any two route
    /// attributes do).
    fn same_slot(self, other: Kind) -> bool {
        self == other || matches!((self, other), (Kind::Route(_), Kind::Route(_)))
    }
}

/// The attribute's name without a `navree::` qualifier, for messages.
pub(crate) fn short_name(attr: &Attribute) -> String {
    attr.path()
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default()
}

/// Where an attribute was written: its tokens, so an error underlines the
/// whole attribute (a joined span is not available on stable).
#[derive(Clone)]
pub(crate) struct At(TokenStream);

impl At {
    pub(crate) fn of(attr: &Attribute) -> Self {
        At(attr.to_token_stream())
    }

    /// The span of the attribute's first token (`#`).
    pub(crate) fn span(&self) -> proc_macro2::Span {
        self.0
            .clone()
            .into_iter()
            .next()
            .map_or_else(proc_macro2::Span::call_site, |token| token.span())
    }

    /// An error pointing at the attribute.
    pub(crate) fn error(&self, message: impl Display) -> syn::Error {
        syn::Error::new_spanned(&self.0, message)
    }
}

/// `#[get("/p")]` and friends.
pub(crate) struct Route {
    /// The upper-case method, e.g. `GET`.
    pub(crate) method: String,
    /// The path relative to the controller (`!`-prefixed when absolute).
    pub(crate) path: LitStr,
}

/// An action or a subject in `#[can(..)]`.
#[derive(Clone)]
pub(crate) enum Name {
    /// A marker type: `Update` (`ActionMarker::NAME`) or `Deal`
    /// (`Subject::SUBJECT`).
    Type(Box<Type>),
    /// A string used as is: `"publish"`.
    Literal(LitStr),
}

impl Parse for Name {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            Ok(Name::Literal(non_empty(input.parse()?)?))
        } else {
            Ok(Name::Type(input.parse()?))
        }
    }
}

/// `#[can(Action, Subject)]` / `#[can(Action, Subject, from = "id")]`.
#[derive(Clone)]
pub(crate) struct Can {
    pub(crate) action: Name,
    pub(crate) subject: Name,
    /// The path parameter holding the record id.
    pub(crate) from: Option<LitStr>,
}

impl Can {
    /// Whether the generated code names navree-access (for marker types).
    pub(crate) fn uses_markers(&self) -> bool {
        matches!(self.action, Name::Type(_)) || matches!(self.subject, Name::Type(_))
    }

    /// Parses the arguments of `#[can(..)]` written at `at`. A malformed
    /// attribute gets one error underlining all of it.
    pub(crate) fn parse_args(attr: &Attribute, at: &At) -> syn::Result<Self> {
        let expected = || {
            at.error("expected #[can(Action, Subject)] or #[can(Action, Subject, from = \"id\")]")
        };
        if !matches!(attr.meta, Meta::List(_)) {
            return Err(expected());
        }
        attr.parse_args_with(|input: ParseStream| {
            let action = input.parse().map_err(|_| expected())?;
            input.parse::<Token![,]>().map_err(|_| expected())?;
            let subject = input.parse().map_err(|_| expected())?;
            let mut from = None;
            if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
                let key: Ident = input.parse().map_err(|_| expected())?;
                if key != "from" {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown #[can] key `{key}`: expected `from = \"id\"`"),
                    ));
                }
                input.parse::<Token![=]>().map_err(|_| expected())?;
                from = Some(non_empty(input.parse().map_err(|_| expected())?)?);
                input.parse::<Option<Token![,]>>()?;
            }
            if !input.is_empty() {
                return Err(expected());
            }
            Ok(Can {
                action,
                subject,
                from,
            })
        })
    }
}

/// `#[throttle(limit = 10, per = "1m")]`.
#[derive(Clone, Copy)]
pub(crate) struct Throttle {
    pub(crate) limit: u32,
    pub(crate) per_millis: u64,
}

impl Throttle {
    fn parse_args(attr: &Attribute) -> syn::Result<Self> {
        let expected = || At::of(attr).error("expected #[throttle(limit = 10, per = \"1m\")]");
        if !matches!(attr.meta, Meta::List(_)) {
            return Err(expected());
        }
        let mut limit = None;
        let mut per = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("limit") {
                let value: LitInt = meta.value()?.parse()?;
                match value.base10_parse::<u32>() {
                    Ok(parsed) if parsed > 0 => limit = Some(parsed),
                    _ => {
                        return Err(syn::Error::new(
                            value.span(),
                            "the limit must be a whole number from 1 to 4294967295",
                        ));
                    }
                }
            } else if meta.path.is_ident("per") {
                per = Some(duration_millis(&meta.value()?.parse()?)?);
            } else {
                return Err(meta.error(format!(
                    "unknown #[throttle] key `{}`: expected `limit` or `per`",
                    key_name(&meta.path)
                )));
            }
            Ok(())
        })?;
        match (limit, per) {
            (Some(limit), Some(per_millis)) => Ok(Throttle { limit, per_millis }),
            _ => Err(expected()),
        }
    }
}

/// `#[requires_flag(NewPipeline)]` / `#[requires_flag(NewPipeline, status = 403)]`.
pub(crate) struct RequiresFlag {
    /// The flag type.
    pub(crate) flag: Box<Type>,
    /// The status while the flag is off: 404 (the default) or 403.
    pub(crate) status: u16,
    /// Where it was written.
    pub(crate) at: At,
}

impl RequiresFlag {
    fn parse_args(attr: &Attribute) -> syn::Result<Self> {
        let at = At::of(attr);
        let expected = || {
            at.error(
                "expected #[requires_flag(NewPipeline)] or \
                 #[requires_flag(NewPipeline, status = 403)]",
            )
        };
        if !matches!(attr.meta, Meta::List(_)) {
            return Err(expected());
        }
        let (flag, status) = attr.parse_args_with(|input: ParseStream| {
            let flag: Type = input.parse().map_err(|_| expected())?;
            let mut status = 404;
            if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
                let key: Ident = input.parse().map_err(|_| expected())?;
                if key != "status" {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown #[requires_flag] key `{key}`: expected `status = 403`"),
                    ));
                }
                input.parse::<Token![=]>().map_err(|_| expected())?;
                let value: LitInt = input.parse().map_err(|_| expected())?;
                status = match value.base10_parse::<u16>() {
                    Ok(status @ (403 | 404)) => status,
                    _ => {
                        return Err(syn::Error::new(
                            value.span(),
                            "a route behind a flag answers 404 (the default, as if it did not \
                             exist) or 403 while the flag is off",
                        ));
                    }
                };
                input.parse::<Option<Token![,]>>()?;
            }
            if input.is_empty() {
                Ok((flag, status))
            } else {
                Err(expected())
            }
        })?;
        Ok(RequiresFlag {
            flag: Box::new(flag),
            status,
            at,
        })
    }
}

/// A duration literal (`"500ms"`, `"5s"`, `"1m"`, `"1h"`) in milliseconds;
/// the error points at the literal.
pub(crate) fn duration_millis(value: &LitStr) -> syn::Result<u64> {
    duration::parse_millis(&value.value()).map_err(|message| syn::Error::new(value.span(), message))
}

/// `#[api(..)]`: OpenAPI metadata.
#[derive(Default)]
pub(crate) struct Api {
    pub(crate) summary: Option<LitStr>,
    pub(crate) description: Option<LitStr>,
    pub(crate) tags: Vec<LitStr>,
    pub(crate) request: Option<Type>,
    pub(crate) response: Option<Type>,
    pub(crate) responses: Vec<u16>,
    pub(crate) status: Option<u16>,
}

/// The keys `#[api]` accepts, for error messages.
const API_KEYS: &str =
    "`summary`, `description`, `tag`, `tags`, `request`, `response`, `responses` or `status`";

impl Api {
    fn parse_args(attr: &Attribute) -> syn::Result<Self> {
        let mut api = Api::default();
        let mut seen: Vec<String> = Vec::new();
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            if key != "tag" && seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            api.parse_key(&key, &meta)?;
            seen.push(key);
            Ok(())
        })?;
        Ok(api)
    }

    fn parse_key(&mut self, key: &str, meta: &ParseNestedMeta) -> syn::Result<()> {
        match key {
            "summary" => self.summary = Some(non_empty(meta.value()?.parse()?)?),
            "description" => self.description = Some(non_empty(meta.value()?.parse()?)?),
            "tag" => self.tags.push(non_empty(meta.value()?.parse()?)?),
            "tags" => {
                let content;
                syn::parenthesized!(content in meta.input);
                for tag in Punctuated::<LitStr, Token![,]>::parse_terminated(&content)? {
                    self.tags.push(non_empty(tag)?);
                }
            }
            "request" => self.request = Some(meta.value()?.parse()?),
            "response" => self.response = Some(meta.value()?.parse()?),
            "responses" => {
                let content;
                syn::parenthesized!(content in meta.input);
                for status in Punctuated::<LitInt, Token![,]>::parse_terminated(&content)? {
                    self.responses.push(http_status(&status)?);
                }
            }
            "status" => self.status = Some(http_status(&meta.value()?.parse()?)?),
            _ => {
                return Err(meta.error(format!("unknown #[api] key `{key}`: expected {API_KEYS}")));
            }
        }
        Ok(())
    }
}

/// An HTTP status code literal (100 to 599).
fn http_status(value: &LitInt) -> syn::Result<u16> {
    match value.base10_parse::<u16>() {
        Ok(status @ 100..=599) => Ok(status),
        _ => Err(syn::Error::new(
            value.span(),
            "expected an HTTP status code between 100 and 599",
        )),
    }
}

/// Who may call a route, with where each declaration was written.
#[derive(Default)]
pub(crate) struct Access {
    pub(crate) public: Option<At>,
    pub(crate) authenticated: Option<At>,
    pub(crate) roles: Vec<LitStr>,
    /// The first `#[roles]` attribute.
    pub(crate) roles_at: Option<At>,
    pub(crate) permissions: Vec<LitStr>,
    /// The first `#[permissions]` attribute.
    pub(crate) permissions_at: Option<At>,
    pub(crate) can: Option<(Can, At)>,
    /// An access attribute was written but did not parse (already
    /// reported), so "no access declaration" would only add noise.
    pub(crate) malformed: bool,
}

impl Access {
    /// Whether anything is declared (deny by default).
    pub(crate) fn is_declared(&self) -> bool {
        self.public.is_some() || self.restriction().is_some()
    }

    /// The first declaration besides `#[public]`.
    pub(crate) fn restriction(&self) -> Option<(&'static str, &At)> {
        [
            ("#[authenticated]", &self.authenticated),
            ("#[roles]", &self.roles_at),
            ("#[permissions]", &self.permissions_at),
        ]
        .into_iter()
        .find_map(|(name, at)| at.as_ref().map(|at| (name, at)))
        .or(self.can.as_ref().map(|(_, at)| ("#[can]", at)))
    }

    /// `#[public]` together with a restriction (on the route or inherited
    /// from the impl block `shared`): the restriction and where to point. A
    /// conflict entirely within `shared` is the impl block's own error.
    pub(crate) fn public_conflict<'a>(
        &'a self,
        shared: &'a Access,
    ) -> Option<(&'static str, &'a At)> {
        match (&self.public, &shared.public) {
            (Some(public), _) => self
                .restriction()
                .or(shared.restriction().map(|(name, _)| (name, public))),
            (None, Some(_)) => self.restriction(),
            (None, None) => None,
        }
    }
}

/// Everything the helper attributes of one method (or of the impl block)
/// declare.
#[derive(Default)]
pub(crate) struct RouteAttrs {
    pub(crate) route: Option<Route>,
    pub(crate) access: Access,
    /// Guard expressions (`Arc<impl Guard>` or `Arc<dyn Guard>`), in order.
    pub(crate) guards: Vec<Expr>,
    pub(crate) throttle: Option<Throttle>,
    pub(crate) timeout_millis: Option<u64>,
    pub(crate) transactional: bool,
    /// `#[audit(action = "x")]`.
    pub(crate) audit: Option<LitStr>,
    /// `#[name("deals.show")]`.
    pub(crate) name: Option<LitStr>,
    pub(crate) api: Option<Api>,
    /// `#[requires_flag(F)]`.
    pub(crate) requires_flag: Option<RequiresFlag>,
    /// The first helper attribute (not a route attribute), for "only
    /// applies to routes" errors.
    pub(crate) first_helper: Option<(String, At)>,
    /// A route attribute was written but did not parse (already reported).
    pub(crate) malformed_route: bool,
}

/// Where the attributes were written.
#[derive(Clone, Copy)]
pub(crate) enum Site<'a> {
    /// On the impl block: they apply to every route.
    Impl,
    /// On the method with this name.
    Method(&'a Ident),
}

/// What may sit on the impl block, for error messages.
const IMPL_ATTRIBUTES: &str =
    "#[public], #[authenticated], #[roles], #[permissions], #[can], #[guard] and #[throttle]";

impl RouteAttrs {
    /// Removes our attributes from `attrs` and parses them. Other attributes
    /// (`#[doc]`, `#[allow]`, `#[cfg]`, ...) stay. Every problem is recorded
    /// in `errors`.
    pub(crate) fn take(attrs: &mut Vec<Attribute>, site: Site, errors: &mut Errors) -> Self {
        let mut parsed = RouteAttrs::default();
        let mut seen: Vec<(Kind, String)> = Vec::new();
        attrs.retain(|attr| {
            let Some(kind) = Kind::of(attr) else {
                return true;
            };
            let name = short_name(attr);
            let at = At::of(attr);
            if let Err(error) = check_site(kind, &name, site, &seen) {
                errors.push(at.error(error));
                return false;
            }
            seen.push((kind, name.clone()));
            if !matches!(kind, Kind::Route(_)) && parsed.first_helper.is_none() {
                parsed.first_helper = Some((name, at));
            }
            if let Err(error) = parsed.parse(kind, attr) {
                // The attribute was written, just wrong: do not also report
                // it as missing.
                match kind {
                    Kind::Route(_) => parsed.malformed_route = true,
                    kind if kind.is_access() => parsed.access.malformed = true,
                    _ => {}
                }
                errors.push(error);
            }
            false
        });
        parsed
    }

    fn parse(&mut self, kind: Kind, attr: &Attribute) -> syn::Result<()> {
        let at = At::of(attr);
        match kind {
            Kind::Route(method) => {
                let path = match &attr.meta {
                    Meta::Path(path) => LitStr::new("/", path.segments[0].ident.span()),
                    _ => attr.parse_args::<LitStr>().map_err(|_| {
                        at.error(format!("expected a path: #[{method}(\"/deals/{{id}}\")]"))
                    })?,
                };
                self.route = Some(Route {
                    method: method.to_ascii_uppercase(),
                    path,
                });
            }
            Kind::Public => self.access.public = Some(flag(attr)?),
            Kind::Authenticated => self.access.authenticated = Some(flag(attr)?),
            Kind::Roles => {
                let roles = strings(attr, "\"admin\", \"manager\"")?;
                self.access.roles.extend(roles);
                self.access.roles_at.get_or_insert(at);
            }
            Kind::Permissions => {
                let permissions = strings(attr, "\"deals.update\"")?;
                self.access.permissions.extend(permissions);
                self.access.permissions_at.get_or_insert(at);
            }
            Kind::Can => {
                let can = Can::parse_args(attr, &at)?;
                self.access.can = Some((can, at));
            }
            Kind::Api => self.api = Some(Api::parse_args(attr)?),
            Kind::RequiresFlag => self.requires_flag = Some(RequiresFlag::parse_args(attr)?),
            Kind::Throttle => self.throttle = Some(Throttle::parse_args(attr)?),
            Kind::Timeout => {
                let value: LitStr = attr
                    .parse_args()
                    .map_err(|_| at.error("expected a duration: #[timeout(\"5s\")]"))?;
                self.timeout_millis = Some(duration_millis(&value)?);
            }
            Kind::Transactional => {
                flag(attr)?;
                self.transactional = true;
            }
            Kind::Audit => {
                let expected = || at.error("expected #[audit(action = \"invoice.sent\")]");
                if !matches!(attr.meta, Meta::List(_)) {
                    return Err(expected());
                }
                let action = attr.parse_args_with(|input: ParseStream| {
                    let key: Ident = input.parse().map_err(|_| expected())?;
                    if key != "action" {
                        return Err(syn::Error::new(
                            key.span(),
                            format!("unknown #[audit] key `{key}`: expected `action`"),
                        ));
                    }
                    input.parse::<Token![=]>().map_err(|_| expected())?;
                    let action = non_empty(input.parse().map_err(|_| expected())?)?;
                    input.parse::<Option<Token![,]>>()?;
                    if input.is_empty() {
                        Ok(action)
                    } else {
                        Err(expected())
                    }
                })?;
                self.audit = Some(action);
            }
            Kind::Name => {
                let name: LitStr = attr
                    .parse_args()
                    .map_err(|_| at.error("expected a route name: #[name(\"deals.show\")]"))?;
                self.name = Some(non_empty(name)?);
            }
            Kind::Guard => {
                let expected = || at.error("expected a guard: #[guard(ValidSignature::guard())]");
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
        }
        Ok(())
    }

    /// Adds what the impl block declares for every route: its access
    /// declarations add to the route's own, and its guards run first.
    pub(crate) fn inherit(&mut self, shared: &RouteAttrs) {
        let access = &mut self.access;
        let shared_access = &shared.access;
        if access.public.is_none() {
            access.public.clone_from(&shared_access.public);
        }
        if access.authenticated.is_none() {
            access
                .authenticated
                .clone_from(&shared_access.authenticated);
        }
        access
            .roles
            .splice(0..0, shared_access.roles.iter().cloned());
        if access.roles_at.is_none() {
            access.roles_at.clone_from(&shared_access.roles_at);
        }
        access
            .permissions
            .splice(0..0, shared_access.permissions.iter().cloned());
        if access.permissions_at.is_none() {
            access
                .permissions_at
                .clone_from(&shared_access.permissions_at);
        }
        if access.can.is_none() {
            access.can.clone_from(&shared_access.can);
        }
        self.guards.splice(0..0, shared.guards.iter().cloned());
        self.throttle = self.throttle.or(shared.throttle);
    }
}

/// Whether attribute `name` of `kind` may be written at `site`, after the
/// attributes `seen` there.
fn check_site(kind: Kind, name: &str, site: Site, seen: &[(Kind, String)]) -> Result<(), String> {
    if let Site::Impl = site {
        if matches!(kind, Kind::Route(_)) {
            return Err(format!(
                "#[{name}] goes on a method of the impl block, not on the impl block"
            ));
        }
        if !kind.allowed_on_impl() {
            return Err(format!(
                "#[{name}] goes on a route method, not on the impl block: on the impl block \
                 only {IMPL_ATTRIBUTES} apply to every route"
            ));
        }
    }
    if kind.repeatable() {
        return Ok(());
    }
    match (seen.iter().find(|(k, _)| k.same_slot(kind)), site) {
        (Some((Kind::Route(_), first)), Site::Method(method)) => Err(format!(
            "a method is one route: `{method}` already has #[{first}]"
        )),
        (Some(_), _) => Err(format!("duplicate #[{name}]")),
        (None, _) => Ok(()),
    }
}

/// A flag attribute such as `#[public]`: no arguments.
pub(crate) fn flag(attr: &Attribute) -> syn::Result<At> {
    let at = At::of(attr);
    match &attr.meta {
        Meta::Path(_) => Ok(at),
        _ => Err(at.error(format!("#[{}] takes no arguments", short_name(attr)))),
    }
}

/// `#[roles("a", "b")]`: a non-empty list of non-empty strings.
pub(crate) fn strings(attr: &Attribute, example: &str) -> syn::Result<Vec<LitStr>> {
    let expected = || At::of(attr).error(format!("expected #[{}({example})]", short_name(attr)));
    let list = match &attr.meta {
        Meta::List(_) => attr
            .parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)
            .map_err(|_| expected())?,
        _ => return Err(expected()),
    };
    if list.is_empty() {
        return Err(expected());
    }
    list.into_iter().map(non_empty).collect()
}

/// `value`, unless it is empty or blank.
pub(crate) fn non_empty(value: LitStr) -> syn::Result<LitStr> {
    if value.value().trim().is_empty() {
        Err(syn::Error::new(value.span(), "must not be empty"))
    } else {
        Ok(value)
    }
}
