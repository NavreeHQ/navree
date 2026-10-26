//! `#[mcp_server]`: implements `navree_mcp::McpServer` for an impl block
//! whose methods carry `#[mcp_tool]`, `#[mcp_resource]` or `#[mcp_prompt]`.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, FnArg, GenericArgument, Ident, ImplItem, ImplItemFn, Item, ItemImpl,
    Lit, LitStr, Meta, Pat, PathArguments, ReturnType, Type,
};

use crate::attr::{Errors, key_name};
use crate::paths::MCP;
use crate::route::{Access, At, Can, Name, flag, non_empty, strings};

/// Expands `#[mcp_server] impl X { .. }`. Errors come with the best
/// expansion possible (the impl block without our attributes, plus the
/// `impl McpServer` of the items that parsed), so one mistake does not
/// cascade.
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
            "#[mcp_server] takes no arguments",
        ));
    }
    let shared = take_attrs(&mut item.attrs, Site::Impl, &mut errors);
    if let (Some(_), Some((restriction, at))) = (&shared.access.public, shared.access.restriction())
    {
        errors.push(at.error(format!(
            "the impl block is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
        )));
    }
    let items = collect_items(&mut item, &shared, &mut errors);
    check_duplicates(&items, &mut errors);
    let mcp = errors.keep(MCP.path(Span::call_site()));
    let mut out = errors
        .finish()
        .err()
        .map(syn::Error::into_compile_error)
        .unwrap_or_default();
    match mcp {
        Some(mcp) => out.extend(generate(&item, &items, &mcp)),
        None => out.extend(item.into_token_stream()),
    }
    out
}

/// `#[mcp_tool]`, `#[mcp_resource]` and `#[mcp_prompt]` outside a
/// `#[mcp_server]` impl block: an error, plus the item unchanged.
pub(crate) fn outside(name: &str, item: TokenStream) -> TokenStream {
    let message = format!("#[{name}] only works on a method of an impl block marked #[mcp_server]");
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
                "#[mcp_server] goes on an inherent impl block (`impl CrmMcp { .. }`), not on a \
                 trait impl",
            )),
        },
        other => Err(syn::Error::new(
            other.span(),
            "#[mcp_server] goes on an impl block: `#[mcp_server] impl CrmMcp { .. }`",
        )),
    }
}

/// The end of the "public and restricted" error.
const PUBLIC_CONFLICT: &str = "a public item needs no authorization; remove one of them";

/// The middle of the "no access declaration" error.
const DECLARE_ACCESS: &str =
    "add #[public], #[authenticated], #[roles], #[permissions] or #[can] (deny by default)";

/// One of the attributes `#[mcp_server]` understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Tool,
    Resource,
    Prompt,
    Public,
    Authenticated,
    Roles,
    Permissions,
    Can,
}

impl Kind {
    /// The attribute `attr` is, when it is one of ours: a plain name or one
    /// qualified by the facade or a navree crate.
    fn of(attr: &Attribute) -> Option<Kind> {
        let path = attr.path();
        let ours = match path.segments.len() {
            1 => path.leading_colon.is_none(),
            2 | 3 => matches!(
                path.segments[0].ident.to_string().as_str(),
                "navree" | "navree_macros" | "navree_mcp" | "mcp"
            ),
            _ => false,
        };
        if !ours {
            return None;
        }
        Some(match path.segments.last()?.ident.to_string().as_str() {
            "mcp_tool" => Kind::Tool,
            "mcp_resource" => Kind::Resource,
            "mcp_prompt" => Kind::Prompt,
            "public" => Kind::Public,
            "authenticated" => Kind::Authenticated,
            "roles" => Kind::Roles,
            "permissions" => Kind::Permissions,
            "can" => Kind::Can,
            _ => return None,
        })
    }

    fn is_item(self) -> bool {
        matches!(self, Kind::Tool | Kind::Resource | Kind::Prompt)
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Tool => "mcp_tool",
            Kind::Resource => "mcp_resource",
            Kind::Prompt => "mcp_prompt",
            Kind::Public => "public",
            Kind::Authenticated => "authenticated",
            Kind::Roles => "roles",
            Kind::Permissions => "permissions",
            Kind::Can => "can",
        }
    }
}

#[derive(Clone, Copy)]
enum Site<'a> {
    Impl,
    Method(&'a Ident),
}

/// `#[mcp_tool(..)]`.
#[derive(Default)]
struct ToolArgs {
    name: Option<LitStr>,
    title: Option<LitStr>,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

/// `#[mcp_resource("uri", ..)]`.
struct ResourceArgs {
    uri: LitStr,
    name: Option<LitStr>,
    title: Option<LitStr>,
    mime: Option<LitStr>,
}

/// `#[mcp_prompt(..)]`.
#[derive(Default)]
struct PromptArgs {
    name: Option<LitStr>,
    title: Option<LitStr>,
}

enum ItemArgs {
    Tool(ToolArgs),
    Resource(ResourceArgs),
    Prompt(PromptArgs),
}

impl ItemArgs {
    fn kind(&self) -> &'static str {
        match self {
            ItemArgs::Tool(_) => "tool",
            ItemArgs::Resource(_) => "resource",
            ItemArgs::Prompt(_) => "prompt",
        }
    }

    fn explicit_name(&self) -> Option<&LitStr> {
        match self {
            ItemArgs::Tool(args) => args.name.as_ref(),
            ItemArgs::Resource(args) => args.name.as_ref(),
            ItemArgs::Prompt(args) => args.name.as_ref(),
        }
    }
}

/// The attributes of one method (or of the impl block).
#[derive(Default)]
struct Attrs {
    item: Option<(ItemArgs, At)>,
    access: Access,
    /// The first access attribute, for "only applies to items" errors.
    first_access: Option<(String, At)>,
    malformed_item: bool,
}

/// Removes our attributes from `attrs` and parses them.
fn take_attrs(attrs: &mut Vec<Attribute>, site: Site, errors: &mut Errors) -> Attrs {
    let mut parsed = Attrs::default();
    let mut seen: Vec<Kind> = Vec::new();
    attrs.retain(|attr| {
        let Some(kind) = Kind::of(attr) else {
            return true;
        };
        let at = At::of(attr);
        if let Site::Impl = site
            && kind.is_item()
        {
            errors.push(at.error(format!(
                "#[{}] goes on a method of the impl block, not on the impl block",
                kind.name()
            )));
            return false;
        }
        let repeatable = matches!(kind, Kind::Roles | Kind::Permissions);
        if !repeatable {
            let clash = seen
                .iter()
                .find(|k| **k == kind || (k.is_item() && kind.is_item()));
            if let Some(first) = clash {
                let message = match site {
                    Site::Method(method) if first.is_item() && kind.is_item() => format!(
                        "a method is one tool, resource or prompt: `{method}` already has #[{}]",
                        first.name()
                    ),
                    _ => format!("duplicate #[{}]", kind.name()),
                };
                errors.push(at.error(message));
                return false;
            }
        }
        seen.push(kind);
        if !kind.is_item() && parsed.first_access.is_none() {
            parsed.first_access = Some((kind.name().to_owned(), at.clone()));
        }
        if let Err(error) = parsed.parse(kind, attr, &at) {
            if kind.is_item() {
                parsed.malformed_item = true;
            } else {
                parsed.access.malformed = true;
            }
            errors.push(error);
        }
        false
    });
    parsed
}

impl Attrs {
    fn parse(&mut self, kind: Kind, attr: &Attribute, at: &At) -> syn::Result<()> {
        match kind {
            Kind::Tool => self.item = Some((ItemArgs::Tool(tool_args(attr)?), at.clone())),
            Kind::Resource => {
                self.item = Some((ItemArgs::Resource(resource_args(attr, at)?), at.clone()));
            }
            Kind::Prompt => self.item = Some((ItemArgs::Prompt(prompt_args(attr)?), at.clone())),
            Kind::Public => self.access.public = Some(flag(attr)?),
            Kind::Authenticated => self.access.authenticated = Some(flag(attr)?),
            Kind::Roles => {
                let roles = strings(attr, "\"admin\", \"manager\"")?;
                self.access.roles.extend(roles);
                self.access.roles_at.get_or_insert(at.clone());
            }
            Kind::Permissions => {
                let permissions = strings(attr, "\"deals.update\"")?;
                self.access.permissions.extend(permissions);
                self.access.permissions_at.get_or_insert(at.clone());
            }
            Kind::Can => self.access.can = Some((Can::parse_args(attr, at)?, at.clone())),
        }
        Ok(())
    }

    /// Adds the impl block's access declarations.
    fn inherit(&mut self, shared: &Attrs) {
        let access = &mut self.access;
        let shared = &shared.access;
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

/// Parses `key = "value"` keys and flag keys of an item attribute.
fn parse_keys(
    attr: &Attribute,
    mut f: impl FnMut(&str, &ParseNestedMeta) -> syn::Result<bool>,
    expected: &str,
) -> syn::Result<()> {
    if matches!(attr.meta, Meta::Path(_)) {
        return Ok(());
    }
    let mut seen = Vec::new();
    attr.parse_nested_meta(|meta| {
        let key = key_name(&meta.path);
        if seen.contains(&key) {
            return Err(meta.error(format!("duplicate `{key}` key")));
        }
        if !f(&key, &meta)? {
            return Err(meta.error(format!(
                "unknown #[{}] key `{key}`: expected {expected}",
                attr.path()
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default()
            )));
        }
        seen.push(key);
        Ok(())
    })
}

fn string_value(meta: &ParseNestedMeta) -> syn::Result<LitStr> {
    non_empty(meta.value()?.parse()?)
}

fn tool_args(attr: &Attribute) -> syn::Result<ToolArgs> {
    let mut args = ToolArgs::default();
    parse_keys(
        attr,
        |key, meta| {
            match key {
                "name" => args.name = Some(string_value(meta)?),
                "title" => args.title = Some(string_value(meta)?),
                "read_only" => args.read_only = true,
                "destructive" => args.destructive = true,
                "idempotent" => args.idempotent = true,
                "open_world" => args.open_world = true,
                _ => return Ok(false),
            }
            Ok(true)
        },
        "`name`, `title`, `read_only`, `destructive`, `idempotent` or `open_world`",
    )?;
    if let Some(name) = &args.name {
        check_tool_name(name)?;
    }
    Ok(args)
}

fn prompt_args(attr: &Attribute) -> syn::Result<PromptArgs> {
    let mut args = PromptArgs::default();
    parse_keys(
        attr,
        |key, meta| {
            match key {
                "name" => args.name = Some(string_value(meta)?),
                "title" => args.title = Some(string_value(meta)?),
                _ => return Ok(false),
            }
            Ok(true)
        },
        "`name` or `title`",
    )?;
    Ok(args)
}

fn resource_args(attr: &Attribute, at: &At) -> syn::Result<ResourceArgs> {
    let expected = || {
        at.error("expected #[mcp_resource(\"crm://deals/{id}\")], optionally with `name`, `title` and `mime`")
    };
    let Meta::List(list) = &attr.meta else {
        return Err(expected());
    };
    // The URI comes first, unnamed: `#[mcp_resource("crm://x", mime = "..")]`.
    let tokens = list.tokens.clone();
    let mut iter = tokens.clone().into_iter().peekable();
    let (mut uri, rest): (Option<LitStr>, TokenStream) = match iter.peek() {
        Some(proc_macro2::TokenTree::Literal(_)) => {
            let first: TokenStream = iter.next().into_iter().collect();
            let literal: LitStr = syn::parse2(first).map_err(|_| expected())?;
            let mut rest: Vec<proc_macro2::TokenTree> = iter.collect();
            if let Some(proc_macro2::TokenTree::Punct(p)) = rest.first()
                && p.as_char() == ','
            {
                rest.remove(0);
            }
            (Some(non_empty(literal)?), rest.into_iter().collect())
        }
        _ => (None, tokens),
    };
    let mut name = None;
    let mut title = None;
    let mut mime = None;
    if !rest.is_empty() {
        let parser = syn::meta::parser(|meta| {
            let key = key_name(&meta.path);
            match key.as_str() {
                "name" => name = Some(string_value(&meta)?),
                "title" => title = Some(string_value(&meta)?),
                "mime" => mime = Some(string_value(&meta)?),
                "uri" => uri = Some(string_value(&meta)?),
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[mcp_resource] key `{key}`: expected `name`, `title` or `mime`"
                    )));
                }
            }
            Ok(())
        });
        syn::parse::Parser::parse2(parser, rest)?;
    }
    let uri = uri.ok_or_else(expected)?;
    check_uri(&uri)?;
    Ok(ResourceArgs {
        uri,
        name,
        title,
        mime,
    })
}

/// MCP tool names: 1 to 128 of `A-Z a-z 0-9 _ - .`.
fn check_tool_name(name: &LitStr) -> syn::Result<()> {
    let value = name.value();
    let valid = (1..=128).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if valid {
        Ok(())
    } else {
        Err(syn::Error::new(
            name.span(),
            "a tool name is 1 to 128 letters, digits, `_`, `-` or `.`",
        ))
    }
}

/// The variables of a resource URI template, or an error explaining what
/// is wrong with it.
fn uri_variables(uri: &str) -> Result<Vec<String>, String> {
    if !uri.contains(':') {
        return Err(format!(
            "resource URI `{uri}` has no scheme: write it like `crm://deals/{{id}}`"
        ));
    }
    let mut variables = Vec::new();
    let mut rest = uri;
    while let Some(open) = rest.find('{') {
        let close = rest[open..]
            .find('}')
            .map(|i| open + i)
            .ok_or_else(|| format!("resource URI `{uri}` has an unclosed `{{`"))?;
        let name = &rest[open + 1..close];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "resource URI `{uri}`: `{{{name}}}` is not a variable name (letters, digits and `_`)"
            ));
        }
        if variables.iter().any(|v| v == name) {
            return Err(format!(
                "resource URI `{uri}` names the variable `{name}` twice"
            ));
        }
        variables.push(name.to_owned());
        rest = &rest[close + 1..];
    }
    if rest.contains('}') {
        return Err(format!("resource URI `{uri}` has an unopened `}}`"));
    }
    Ok(variables)
}

fn check_uri(uri: &LitStr) -> syn::Result<()> {
    uri_variables(&uri.value())
        .map(drop)
        .map_err(|message| syn::Error::new(uri.span(), message))
}

/// A method parameter after `&self`.
enum Param {
    /// `&McpContext`.
    Context,
    /// `Authorized<E, A>`: the record of the item's `from` check.
    Record(Box<Type>),
    /// The arguments struct (tools and prompts).
    Args(Box<Type>),
    /// A URI variable (resources): the parameter is named like it.
    Variable(Ident, Box<Type>),
}

/// How a method's result is sent.
enum Output {
    Json,
    Contents,
    Messages,
}

/// One item method.
struct ItemFn {
    ident: Ident,
    args: ItemArgs,
    access: Access,
    params: Vec<Param>,
    output: Output,
    description: Option<String>,
    cfgs: Vec<Attribute>,
}

impl ItemFn {
    /// The item's name: the explicit one, else the method name.
    fn name(&self) -> LitStr {
        self.args
            .explicit_name()
            .cloned()
            .unwrap_or_else(|| LitStr::new(&self.ident.to_string(), self.ident.span()))
    }
}

fn collect_items(item: &mut ItemImpl, shared: &Attrs, errors: &mut Errors) -> Vec<ItemFn> {
    let mut items = Vec::new();
    for impl_item in &mut item.items {
        let ImplItem::Fn(method) = impl_item else {
            continue;
        };
        let ident = method.sig.ident.clone();
        let mut attrs = take_attrs(&mut method.attrs, Site::Method(&ident), errors);
        let Some((args, at)) = attrs.item.take() else {
            if let (Some((name, at)), false) = (&attrs.first_access, attrs.malformed_item) {
                errors.push(at.error(format!(
                    "#[{name}] only applies to MCP items: `{ident}` has no #[mcp_tool], \
                     #[mcp_resource] or #[mcp_prompt]"
                )));
            }
            continue;
        };
        if let (Some((_, can_at)), Some(_)) = (&attrs.access.can, &shared.access.can) {
            errors.push(can_at.error(format!(
                "`{ident}` has #[can] here and on the impl block: an item checks one ability"
            )));
        }
        if let Some((restriction, conflict_at)) = attrs.access.public_conflict(&shared.access) {
            errors.push(conflict_at.error(format!(
                "`{ident}` is #[public] but also declares {restriction}: {PUBLIC_CONFLICT}"
            )));
        }
        attrs.inherit(shared);
        if !attrs.access.is_declared() && !attrs.access.malformed {
            errors.push(syn::Error::new(
                ident.span(),
                format!(
                    "MCP {} `{ident}` has no access declaration: {DECLARE_ACCESS}",
                    args.kind()
                ),
            ));
        }
        let Some(params) = errors.keep(check_signature(method, &args, &attrs.access, &at)) else {
            continue;
        };
        let Some(output) = errors.keep(output_kind(method, &args)) else {
            continue;
        };
        items.push(ItemFn {
            ident,
            args,
            access: attrs.access,
            params,
            output,
            description: doc_comment(&method.attrs),
            cfgs: method
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("cfg"))
                .cloned()
                .collect(),
        });
    }
    items
}

/// Two items of a kind with the same name (`#[cfg]` items aside).
fn check_duplicates(items: &[ItemFn], errors: &mut Errors) {
    let mut seen: Vec<(&str, String, &Ident)> = Vec::new();
    for item in items.iter().filter(|item| item.cfgs.is_empty()) {
        let kind = item.args.kind();
        let name = item.name().value();
        match seen.iter().find(|(k, n, _)| *k == kind && *n == name) {
            Some((_, _, first)) => errors.push(syn::Error::new(
                item.ident.span(),
                format!(
                    "`{}` and `{first}` are both the MCP {kind} `{name}`",
                    item.ident
                ),
            )),
            None => seen.push((kind, name, &item.ident)),
        }
    }
}

/// An item is a non-generic `async fn(&self, ..)` returning a `Result`;
/// its parameters are classified.
fn check_signature(
    method: &ImplItemFn,
    args: &ItemArgs,
    access: &Access,
    at: &At,
) -> syn::Result<Vec<Param>> {
    let sig = &method.sig;
    let ident = &sig.ident;
    let mut errors = Errors::default();
    if sig.asyncness.is_none() {
        errors.push(syn::Error::new(
            ident.span(),
            format!("MCP item `{ident}` must be an `async fn`"),
        ));
    }
    match sig.receiver() {
        Some(receiver) if matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, None)) => {}
        Some(receiver) => errors.push(syn::Error::new_spanned(
            receiver,
            format!("MCP item `{ident}` must take `&self`"),
        )),
        None => errors.push(syn::Error::new(
            ident.span(),
            format!("MCP item `{ident}` must take `&self`"),
        )),
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &sig.generics,
            format!("MCP item `{ident}` cannot be generic"),
        ));
    }
    let from = access
        .can
        .as_ref()
        .and_then(|(can, _)| can.from.as_ref().map(LitStr::value));
    let variables = match args {
        ItemArgs::Resource(resource) => uri_variables(&resource.uri.value()).unwrap_or_default(),
        _ => Vec::new(),
    };
    if let (ItemArgs::Resource(resource), Some(from)) = (args, &from)
        && !variables.contains(from)
    {
        errors.push(at.error(format!(
            "`from = \"{from}\"` names no variable of the resource URI `{}`: the record id must \
             come from a variable such as `{{{from}}}`",
            resource.uri.value()
        )));
    }
    let mut params = Vec::new();
    for input in &sig.inputs {
        let FnArg::Typed(typed) = input else {
            continue;
        };
        let ty = &*typed.ty;
        if is_context(ty) {
            if params.iter().any(|p| matches!(p, Param::Context)) {
                errors.push(syn::Error::new_spanned(
                    ty,
                    "only one `&McpContext` parameter",
                ));
            }
            params.push(Param::Context);
        } else if last_segment(ty).is_some_and(|segment| segment == "Authorized") {
            if from.is_none() {
                errors.push(syn::Error::new_spanned(
                    ty,
                    format!(
                        "`{ident}` takes an `Authorized` record but declares no \
                         #[can(Action, Subject, from = \"..\")] to load it"
                    ),
                ));
            }
            params.push(Param::Record(Box::new(ty.clone())));
        } else if let ItemArgs::Resource(resource) = args {
            match &*typed.pat {
                Pat::Ident(pat) if variables.contains(&pat.ident.to_string()) => {
                    params.push(Param::Variable(pat.ident.clone(), Box::new(ty.clone())));
                }
                pat => errors.push(syn::Error::new_spanned(
                    pat,
                    format!(
                        "a resource parameter is `&McpContext`, an `Authorized` record or a \
                         variable of `{}` (by name)",
                        resource.uri.value()
                    ),
                )),
            }
        } else {
            if params.iter().any(|p| matches!(p, Param::Args(_))) {
                errors.push(syn::Error::new_spanned(
                    ty,
                    format!(
                        "an MCP {} takes one arguments value: put every argument in one struct",
                        args.kind()
                    ),
                ));
            }
            params.push(Param::Args(Box::new(ty.clone())));
        }
    }
    errors.finish().map(|()| params)
}

/// `&McpContext` (any path ending in `McpContext`).
fn is_context(ty: &Type) -> bool {
    match ty {
        Type::Reference(reference) => {
            last_segment(&reference.elem).is_some_and(|s| s == "McpContext")
        }
        _ => false,
    }
}

/// The last path segment of a type (`Authorized` for `Authorized<Deal, Read>`).
fn last_segment(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// How the method's `Result` is sent: resource contents, prompt messages
/// or a serialized value.
fn output_kind(method: &ImplItemFn, args: &ItemArgs) -> syn::Result<Output> {
    let ident = &method.sig.ident;
    let ReturnType::Type(_, ty) = &method.sig.output else {
        return Err(syn::Error::new(
            ident.span(),
            format!("MCP item `{ident}` must return a `Result`"),
        ));
    };
    let Type::Path(path) = &**ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "an MCP item returns a `Result`",
        ));
    };
    let Some(last) = path.path.segments.last().filter(|s| s.ident == "Result") else {
        return Err(syn::Error::new_spanned(
            ty,
            format!("MCP item `{ident}` must return a `Result` (`navree::Result<T>`)"),
        ));
    };
    if let ItemArgs::Prompt(_) = args {
        return Ok(Output::Messages);
    }
    let ok = match &last.arguments {
        PathArguments::AngleBracketed(arguments) => {
            arguments.args.iter().find_map(|arg| match arg {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
        }
        _ => None,
    };
    let contents = ok.is_some_and(|ok| match last_segment(ok).as_deref() {
        Some("ResourceContents") => true,
        Some("Vec") => match ok {
            Type::Path(path) => match &path.path.segments.last().map(|s| &s.arguments) {
                Some(PathArguments::AngleBracketed(arguments)) => arguments.args.iter().any(|arg| {
                    matches!(arg, GenericArgument::Type(inner) if last_segment(inner).as_deref() == Some("ResourceContents"))
                }),
                _ => false,
            },
            _ => false,
        },
        _ => false,
    });
    Ok(if contents && matches!(args, ItemArgs::Resource(_)) {
        Output::Contents
    } else {
        Output::Json
    })
}

/// The doc comment, each line without its leading space, trimmed.
fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(value) => match &value.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_owned())
        .collect();
    let text = lines.join("\n").trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// The impl block plus its `impl McpServer`.
fn generate(item: &ItemImpl, items: &[ItemFn], mcp: &TokenStream) -> TokenStream {
    let list = Ident::new("items", Span::mixed_site());
    let pushes = items.iter().map(|item_fn| {
        let cfgs = &item_fn.cfgs;
        let item = item_expr(item_fn, mcp);
        quote!(#(#cfgs)* #list.push(#item);)
    });
    let self_ty = &item.self_ty;
    let (impl_generics, _, where_clause) = item.generics.split_for_impl();
    quote! {
        #item

        impl #impl_generics #mcp::McpServer for #self_ty #where_clause {
            fn items(self: ::std::sync::Arc<Self>) -> ::std::vec::Vec<#mcp::McpItem> {
                let mut #list = ::std::vec::Vec::new();
                #(#pushes)*
                #list
            }
        }
    }
}

/// The `McpItem` expression of one method.
fn item_expr(item: &ItemFn, mcp: &TokenStream) -> TokenStream {
    let local = |name: &str| Ident::new(name, Span::mixed_site());
    let (this, call) = (local("this"), local("call"));
    let ident = &item.ident;
    let name = item.name();
    let mut def = match &item.args {
        ItemArgs::Tool(args) => {
            let mut def = quote!(#mcp::__private::ToolDef::new(#name));
            if let Some(title) = &args.title {
                def.extend(quote!(.title(#title)));
            }
            for (on, hint) in [
                (args.read_only, "read_only"),
                (args.destructive, "destructive"),
                (args.idempotent, "idempotent"),
                (args.open_world, "open_world"),
            ] {
                if on {
                    let hint = Ident::new(hint, Span::call_site());
                    def.extend(quote!(.#hint()));
                }
            }
            def
        }
        ItemArgs::Resource(args) => {
            let uri = &args.uri;
            let mut def = quote!(#mcp::__private::ResourceDef::new(#uri, #name));
            if let Some(title) = &args.title {
                def.extend(quote!(.title(#title)));
            }
            if let Some(mime) = &args.mime {
                def.extend(quote!(.mime_type(#mime)));
            }
            def
        }
        ItemArgs::Prompt(args) => {
            let mut def = quote!(#mcp::__private::PromptDef::new(#name));
            if let Some(title) = &args.title {
                def.extend(quote!(.title(#title)));
            }
            def
        }
    };
    if let Some(description) = &item.description {
        def.extend(quote!(.description(#description)));
    }
    for param in &item.params {
        if let Param::Args(ty) = param
            && !matches!(item.args, ItemArgs::Resource(_))
        {
            def.extend(quote_spanned!(ty.span()=> .input::<#ty>()));
        }
    }
    def.extend(access_calls(&item.access, mcp));

    let mut bindings = TokenStream::new();
    let mut arguments = Vec::new();
    for (index, param) in item.params.iter().enumerate() {
        let var = local(&format!("arg{index}"));
        match param {
            Param::Context => arguments.push(quote!(#call.context())),
            Param::Args(ty) => {
                bindings.extend(
                    quote_spanned!(ty.span()=> let #var: #ty = #call.args::<#ty>().await?;),
                );
                arguments.push(quote!(#var));
            }
            Param::Record(ty) => {
                bindings.extend(quote_spanned!(ty.span()=>
                    let #var: #ty = <#ty as #mcp::__private::FromRecord>::from_record(&#call)?;
                ));
                arguments.push(quote!(#var));
            }
            Param::Variable(name, ty) => {
                let key = LitStr::new(&name.to_string(), name.span());
                bindings.extend(quote_spanned!(ty.span()=>
                    let #var: #ty = #mcp::__private::param::<#ty>(&#call, #key)?;
                ));
                arguments.push(quote!(#var));
            }
        }
    }
    let (constructor, output) = match (&item.args, &item.output) {
        (ItemArgs::Tool(_), _) => (quote!(tool), quote!(json_output)),
        (ItemArgs::Resource(_), Output::Contents) => (quote!(resource), quote!(contents_output)),
        (ItemArgs::Resource(_), _) => (quote!(resource), quote!(json_output)),
        (ItemArgs::Prompt(_), _) => (quote!(prompt), quote!(prompt_output)),
    };
    quote! {
        {
            let #this = ::std::sync::Arc::clone(&self);
            #mcp::McpItem::#constructor(#def, move |#call: #mcp::__private::Call| {
                let #this = ::std::sync::Arc::clone(&#this);
                async move {
                    #bindings
                    #mcp::__private::#output(Self::#ident(&*#this, #(#arguments),*).await)
                }
            })
        }
    }
}

/// The access builder calls of an item.
fn access_calls(access: &Access, mcp: &TokenStream) -> TokenStream {
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
        let name = |name: &Name, marker: &str, constant: &str| match name {
            Name::Literal(literal) => literal.to_token_stream(),
            Name::Type(ty) => {
                let (marker, constant) = (
                    Ident::new(marker, ty.span()),
                    Ident::new(constant, ty.span()),
                );
                quote_spanned!(ty.span()=> <#ty as #mcp::__private::#marker>::#constant)
            }
        };
        let action = name(&can.action, "ActionMarker", "NAME");
        let subject = name(&can.subject, "Subject", "SUBJECT");
        let from = match &can.from {
            Some(from) => quote!(::core::option::Option::Some(#from)),
            None => quote!(::core::option::Option::None),
        };
        out.extend(quote!(.can(#action, #subject, #from)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_variables_are_checked() {
        assert_eq!(uri_variables("crm://deals/{id}").unwrap(), ["id"]);
        assert!(uri_variables("crm://pipeline").unwrap().is_empty());
        assert!(
            uri_variables("deals/{id}")
                .unwrap_err()
                .contains("no scheme")
        );
        assert!(
            uri_variables("crm://{a}/{a}")
                .unwrap_err()
                .contains("twice")
        );
        assert!(
            uri_variables("crm://{a-b}")
                .unwrap_err()
                .contains("variable name")
        );
    }

    #[test]
    fn doc_comments_become_descriptions() {
        let method: ImplItemFn = syn::parse_quote! {
            /// Lists the deals.
            ///
            /// At most 25.
            async fn list(&self) {}
        };
        assert_eq!(
            doc_comment(&method.attrs).as_deref(),
            Some("Lists the deals.\n\nAt most 25.")
        );
    }
}
