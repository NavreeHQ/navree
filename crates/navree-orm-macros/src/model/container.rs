//! The container attributes: `#[model(table = "...", timestamps, ...)]` on
//! the struct.

use proc_macro2::Span;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Attribute, LitBool, LitInt, LitStr, Token};

use super::attributes;
use crate::errors::{Errors, key_name, suggestion};
use crate::model::lifecycle::{self, Lifecycle};

/// How the key gets its value (navree-orm's `KeyKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyType {
    Increments,
    UuidV7,
    UuidV4,
    Ulid,
    Manual,
}

impl KeyType {
    fn parse(text: &LitStr) -> syn::Result<Self> {
        match text.value().as_str() {
            "int" | "integer" | "increments" => Ok(KeyType::Increments),
            "uuid" | "uuid_v7" => Ok(KeyType::UuidV7),
            "uuid_v4" => Ok(KeyType::UuidV4),
            "ulid" => Ok(KeyType::Ulid),
            "manual" => Ok(KeyType::Manual),
            other => Err(syn::Error::new(
                text.span(),
                format!(
                    "unknown key type `{other}`: use \"int\" (the database assigns it), \"uuid\" (UUID v7), \"uuid_v4\", \"ulid\" or \"manual\" (the application sets it)"
                ),
            )),
        }
    }
}

/// The timestamp columns.
#[derive(Clone, Debug, Default)]
pub(crate) struct Timestamps {
    /// Whether `timestamps` was given at all.
    pub(crate) on: bool,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) span: Option<Span>,
}

/// Everything `#[model(...)]` on the struct says.
#[derive(Default)]
pub(crate) struct Container {
    pub(crate) table: Option<LitStr>,
    pub(crate) primary_key: Option<LitStr>,
    pub(crate) key_type: Option<KeyType>,
    pub(crate) timestamps: Timestamps,
    pub(crate) soft_deletes: Option<(String, Span)>,
    /// `observers(..)`, `dispatches(..)`, `broadcasts`, `prunable`, ...
    pub(crate) lifecycle: Lifecycle,
    pub(crate) connection: Option<LitStr>,
    pub(crate) per_page: Option<LitInt>,
    pub(crate) global_scopes: Vec<syn::Expr>,
    /// `touches`, `morph_name`, `with`, `with_count`, `pivot`.
    pub(crate) relations: super::relations::RelationAttrs,
    /// The attribute layer's keys.
    pub(crate) attributes: attributes::ContainerAttrs,
}

/// The keys `#[model(...)]` takes on a struct (the attribute layer's come
/// from [`attributes::CONTAINER_KEYS`]).
const KEYS: &[&str] = &[
    "table",
    "primary_key",
    "key_type",
    "timestamps",
    "soft_deletes",
    "connection",
    "per_page",
    "global_scopes",
    "touches",
    "morph_name",
    "with",
    "with_count",
    "pivot",
];

/// Parses the `#[model(...)]` attributes of the struct.
pub(crate) fn parse(attrs: &[Attribute], errors: &mut Errors) -> Container {
    let mut container = Container::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        errors.keep(attr.parse_nested_meta(|meta| parse_key(&mut container, &meta)));
    }
    container
}

fn parse_key(container: &mut Container, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    let path = &meta.path;
    if path.is_ident("table") {
        container.table = Some(non_empty(meta.value()?.parse()?)?);
    } else if path.is_ident("primary_key") {
        container.primary_key = Some(non_empty(meta.value()?.parse()?)?);
    } else if path.is_ident("key_type") {
        container.key_type = Some(KeyType::parse(&meta.value()?.parse()?)?);
    } else if path.is_ident("timestamps") {
        container.timestamps = Timestamps {
            on: true,
            created_at: Some("created_at".to_owned()),
            updated_at: Some("updated_at".to_owned()),
            span: Some(
                path.get_ident()
                    .map_or_else(Span::call_site, syn::Ident::span),
            ),
        };
        if meta.input.peek(syn::token::Paren) {
            meta.parse_nested_meta(|inner| parse_timestamp(&mut container.timestamps, &inner))?;
        }
    } else if path.is_ident("soft_deletes") {
        let span = path
            .get_ident()
            .map_or_else(Span::call_site, syn::Ident::span);
        let column = if meta.input.peek(Token![=]) {
            non_empty(meta.value()?.parse()?)?.value()
        } else {
            "deleted_at".to_owned()
        };
        container.soft_deletes = Some((column, span));
    } else if lifecycle::parse_key(&mut container.lifecycle, meta)? {
        // An observer, dispatch, broadcast or pruning attribute.
    } else if path.is_ident("connection") {
        container.connection = Some(non_empty(meta.value()?.parse()?)?);
    } else if path.is_ident("per_page") {
        let per_page: LitInt = meta.value()?.parse()?;
        let value: u64 = per_page.base10_parse()?;
        if value == 0 {
            return Err(syn::Error::new(
                per_page.span(),
                "per_page must be at least 1",
            ));
        }
        container.per_page = Some(per_page);
    } else if path.is_ident("global_scopes") {
        let content;
        syn::parenthesized!(content in meta.input);
        let scopes = Punctuated::<syn::Expr, Token![,]>::parse_terminated(&content)?;
        container.global_scopes.extend(scopes);
    } else if super::relations::parse_key(&mut container.relations, meta)? {
        // A relation attribute.
    } else if attributes::parse_container_key(&mut container.attributes, meta)? {
        // An attribute-layer key.
    } else {
        let key = key_name(path);
        let known: Vec<&str> = KEYS
            .iter()
            .chain(lifecycle::KEYS)
            .chain(attributes::CONTAINER_KEYS)
            .copied()
            .collect();
        let hint = suggestion(&key, &known)
            .map(|known| format!(" (did you mean `{known}`?)"))
            .unwrap_or_default();
        return Err(meta.error(format!(
            "unknown model attribute `{key}`{hint}; expected one of: {}",
            known.join(", ")
        )));
    }
    Ok(())
}

/// `created_at = "created_on"` or `updated_at = false` inside
/// `timestamps(...)`.
fn parse_timestamp(timestamps: &mut Timestamps, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    let slot = if meta.path.is_ident("created_at") {
        &mut timestamps.created_at
    } else if meta.path.is_ident("updated_at") {
        &mut timestamps.updated_at
    } else {
        return Err(meta.error(format!(
            "unknown timestamps option `{}`; expected `created_at` or `updated_at` (a column name, or false to turn it off)",
            key_name(&meta.path)
        )));
    };
    let value = meta.value()?;
    if value.peek(LitBool) {
        let flag: LitBool = value.parse()?;
        if flag.value {
            return Err(syn::Error::new(
                flag.span(),
                "name the column (\"...\") or turn it off with false",
            ));
        }
        *slot = None;
    } else {
        *slot = Some(non_empty(value.parse()?)?.value());
    }
    Ok(())
}

fn non_empty(text: LitStr) -> syn::Result<LitStr> {
    if text.value().trim().is_empty() {
        Err(syn::Error::new(text.span(), "this name cannot be empty"))
    } else {
        Ok(text)
    }
}
