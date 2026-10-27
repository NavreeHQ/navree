//! The attribute layer of `#[derive(Model)]`.
//!
//! Container keys: `fillable(..)`, `guarded(..)`, `unguarded` (mass
//! assignment), `hidden(..)`, `visible(..)`, `appends(..)`,
//! `serialize_dates_with = path` (serialization), `encrypter = "..."`
//! (encrypted casts), `resource = Type` (API resources).
//!
//! Field keys: `hidden`, `fillable`, `guarded`, `set_with = path`
//! (mutator), `schema = Type` (OpenAPI), `inbound = path` (inbound cast),
//! `columns(name: Type, ..)` (a field stored across several columns), and
//! the built-in casts that take arguments (`cast = "decimal(2)"`,
//! `cast = "datetime_format(\"%d/%m/%Y\")"`).
//!
//! It also reads what serde says about each field (renames, skips,
//! custom serializers), so serialized records know each field's key.

use heck::ToShoutySnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, LitStr, Token, Type};

use super::{Cast, Column, Model, plain_name};
use crate::errors::{Errors, suggestion};

// -------------------------------------------------------------------------
// Container keys
// -------------------------------------------------------------------------

/// A name in a model-level list: a field (checked when the model compiles)
/// or any text (a column, a JSON path, an output key).
#[derive(Clone)]
pub(crate) enum Name {
    Field(Ident),
    Text(LitStr),
}

impl Parse for Name {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(LitStr) {
            let text: LitStr = input.parse()?;
            if text.value().trim().is_empty() {
                return Err(syn::Error::new(text.span(), "this name cannot be empty"));
            }
            Ok(Name::Text(text))
        } else {
            Ok(Name::Field(Ident::parse_any(input)?))
        }
    }
}

/// The attribute layer's keys on the struct.
#[derive(Default)]
pub(crate) struct ContainerAttrs {
    pub(crate) fillable: Option<(Vec<Name>, Span)>,
    pub(crate) guarded: Option<(Vec<Name>, Span)>,
    pub(crate) unguarded: Option<Span>,
    pub(crate) hidden: Vec<Name>,
    pub(crate) visible: Vec<Name>,
    pub(crate) appends: Vec<Ident>,
    pub(crate) encrypter: Option<LitStr>,
    pub(crate) serialize_dates_with: Option<syn::Path>,
    pub(crate) resource: Option<Type>,
}

/// The attribute layer's keys on the struct.
pub(crate) const CONTAINER_KEYS: &[&str] = &[
    "fillable",
    "guarded",
    "unguarded",
    "hidden",
    "visible",
    "appends",
    "encrypter",
    "serialize_dates_with",
    "resource",
];

fn key_span(meta: &ParseNestedMeta<'_>) -> Span {
    meta.path
        .get_ident()
        .map_or_else(Span::call_site, Ident::span)
}

/// `(a, b, "c")` after a key; `required` names the key for the message
/// when the list is missing.
fn parse_list<T: Parse>(meta: &ParseNestedMeta<'_>, example: &str) -> syn::Result<Vec<T>> {
    if !meta.input.peek(syn::token::Paren) {
        let key = meta
            .path
            .get_ident()
            .map_or_else(String::new, ToString::to_string);
        return Err(meta.error(format!("`{key}` takes a list: {example}")));
    }
    let content;
    syn::parenthesized!(content in meta.input);
    Ok(Punctuated::<T, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

/// A path given as a path or as a string holding one.
fn parse_path(meta: &ParseNestedMeta<'_>) -> syn::Result<syn::Path> {
    let value = meta.value()?;
    if value.peek(LitStr) {
        let text: LitStr = value.parse()?;
        text.parse()
    } else {
        value.parse()
    }
}

/// A type given as a type or as a string holding one.
fn parse_type(meta: &ParseNestedMeta<'_>) -> syn::Result<Type> {
    let value = meta.value()?;
    if value.peek(LitStr) {
        let text: LitStr = value.parse()?;
        text.parse()
    } else {
        value.parse()
    }
}

/// Parses an attribute-layer key on the struct; `Ok(false)` when the key
/// is not one.
pub(crate) fn parse_container_key(
    attrs: &mut ContainerAttrs,
    meta: &ParseNestedMeta<'_>,
) -> syn::Result<bool> {
    let path = &meta.path;
    if path.is_ident("fillable") {
        let span = key_span(meta);
        let names = parse_list(meta, "fillable(title, amount_cents)")?;
        attrs.fillable.get_or_insert_with(|| (Vec::new(), span)).0.extend(names);
    } else if path.is_ident("guarded") {
        let span = key_span(meta);
        let names = parse_list(meta, "guarded(id, owner_id), or guarded() to guard nothing")?;
        attrs.guarded.get_or_insert_with(|| (Vec::new(), span)).0.extend(names);
    } else if path.is_ident("unguarded") {
        attrs.unguarded = Some(key_span(meta));
    } else if path.is_ident("hidden") {
        attrs.hidden.extend(parse_list::<Name>(meta, "hidden(password_hash, \"internal\")")?);
    } else if path.is_ident("visible") {
        attrs.visible.extend(parse_list::<Name>(meta, "visible(id, title)")?);
    } else if path.is_ident("appends") {
        attrs.appends.extend(parse_list::<Ident>(meta, "appends(full_name)")?);
    } else if path.is_ident("encrypter") {
        let name: LitStr = meta.value()?.parse()?;
        if name.value().trim().is_empty() {
            return Err(syn::Error::new(name.span(), "the encrypter's name cannot be empty"));
        }
        attrs.encrypter = Some(name);
    } else if path.is_ident("serialize_dates_with") {
        attrs.serialize_dates_with = Some(parse_path(meta)?);
    } else if path.is_ident("resource") {
        attrs.resource = Some(parse_type(meta)?);
    } else {
        return Ok(false);
    }
    Ok(true)
}

// -------------------------------------------------------------------------
// Field keys
// -------------------------------------------------------------------------

/// The attribute layer's keys on a field.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    pub(crate) hidden: Option<Span>,
    pub(crate) fillable: Option<Span>,
    pub(crate) guarded: Option<Span>,
    pub(crate) set_with: Option<syn::Path>,
    pub(crate) schema: Option<Type>,
    pub(crate) inbound: Option<syn::Path>,
    pub(crate) columns: Option<(Vec<(Ident, Type)>, Span)>,
}

impl FieldAttrs {
    /// The flags that apply to each of the field's columns (`hidden`,
    /// `fillable`, `guarded`).
    pub(crate) fn flags(&self) -> FieldAttrs {
        FieldAttrs {
            hidden: self.hidden,
            fillable: self.fillable,
            guarded: self.guarded,
            ..FieldAttrs::default()
        }
    }
}

/// The attribute layer's keys on a field.
pub(crate) const FIELD_KEYS: &[&str] = &[
    "hidden", "fillable", "guarded", "set_with", "schema", "inbound", "columns",
];

/// `name: Type` inside `columns(..)`.
struct ColumnDecl(Ident, Type);

impl Parse for ColumnDecl {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = Ident::parse_any(input)?;
        input.parse::<Token![:]>()?;
        Ok(ColumnDecl(name, input.parse()?))
    }
}

/// Parses an attribute-layer key on a field; `Ok(false)` when the key is
/// not one.
pub(crate) fn parse_field_key(attrs: &mut FieldAttrs, meta: &ParseNestedMeta<'_>) -> syn::Result<bool> {
    let path = &meta.path;
    if path.is_ident("hidden") {
        attrs.hidden = Some(key_span(meta));
    } else if path.is_ident("fillable") {
        attrs.fillable = Some(key_span(meta));
    } else if path.is_ident("guarded") {
        attrs.guarded = Some(key_span(meta));
    } else if path.is_ident("set_with") {
        attrs.set_with = Some(parse_path(meta)?);
    } else if path.is_ident("schema") {
        attrs.schema = Some(parse_type(meta)?);
    } else if path.is_ident("inbound") {
        attrs.inbound = Some(parse_path(meta)?);
    } else if path.is_ident("columns") {
        let span = key_span(meta);
        let columns: Vec<ColumnDecl> =
            parse_list(meta, "columns(price_cents: i64, price_currency: String)")?;
        if columns.len() < 2 {
            return Err(syn::Error::new(
                span,
                "columns(..) stores a field across several columns: name at least two (`name: Type`)",
            ));
        }
        if columns.len() > 8 {
            return Err(syn::Error::new(span, "a field can be stored across at most eight columns"));
        }
        attrs.columns = Some((
            columns.into_iter().map(|ColumnDecl(name, ty)| (name, ty)).collect(),
            span,
        ));
    } else {
        return Ok(false);
    }
    Ok(true)
}

// -------------------------------------------------------------------------
// Built-in casts
// -------------------------------------------------------------------------

/// The attribute layer's built-in casts.
#[derive(Clone)]
pub(crate) enum BuiltinCast {
    Decimal(u32),
    DateTimeFormat(String),
    UnixTimestamp,
    UnixTimestampMs,
    Encrypted,
    EncryptedJson,
    Hashed,
    BinaryUuid,
    BinaryUlid,
    Vector,
}

/// Every built-in cast, for messages.
pub(crate) const BUILTIN_CAST_NAMES: &str = "\"json\", \"string_enum\", \"int_enum\", \"decimal(N)\", \"datetime_format(\\\"...\\\")\", \"unix_timestamp\", \"unix_timestamp_ms\", \"encrypted\", \"encrypted_json\", \"hashed\", \"binary_uuid\", \"binary_ulid\", \"vector\"";

/// The largest decimal scale (rust_decimal's precision).
const MAX_SCALE: u32 = 28;

/// `text`'s `name(arguments)` form: the arguments.
fn call_arguments<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.strip_prefix(name)?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// A built-in cast written as a string, `None` for any other string.
pub(crate) fn parse_builtin_cast(text: &LitStr) -> syn::Result<Option<BuiltinCast>> {
    let value = text.value();
    let value = value.trim();
    let simple = match value {
        "unix_timestamp" => Some(BuiltinCast::UnixTimestamp),
        "unix_timestamp_ms" => Some(BuiltinCast::UnixTimestampMs),
        "encrypted" => Some(BuiltinCast::Encrypted),
        "encrypted_json" => Some(BuiltinCast::EncryptedJson),
        "hashed" => Some(BuiltinCast::Hashed),
        "binary_uuid" => Some(BuiltinCast::BinaryUuid),
        "binary_ulid" => Some(BuiltinCast::BinaryUlid),
        "vector" => Some(BuiltinCast::Vector),
        "decimal" => {
            return Err(syn::Error::new(
                text.span(),
                "decimal casts take a scale: \"decimal(2)\" keeps two decimal places",
            ));
        }
        "datetime_format" => {
            return Err(syn::Error::new(
                text.span(),
                "datetime_format casts take a chrono format: \"datetime_format(\\\"%d/%m/%Y %H:%M\\\")\"",
            ));
        }
        _ => None,
    };
    if simple.is_some() {
        return Ok(simple);
    }
    if let Some(arguments) = call_arguments(value, "decimal") {
        let scale: u32 = arguments.trim().parse().map_err(|_| {
            syn::Error::new(
                text.span(),
                format!("`{arguments}` is not a scale: write \"decimal(2)\" for two decimal places"),
            )
        })?;
        if scale > MAX_SCALE {
            return Err(syn::Error::new(
                text.span(),
                format!("a decimal keeps at most {MAX_SCALE} decimal places, not {scale}"),
            ));
        }
        return Ok(Some(BuiltinCast::Decimal(scale)));
    }
    if let Some(arguments) = call_arguments(value, "datetime_format") {
        let format: LitStr = syn::parse_str(arguments.trim()).map_err(|_| {
            syn::Error::new(
                text.span(),
                "datetime_format takes the format as a string: \"datetime_format(\\\"%d/%m/%Y %H:%M\\\")\"",
            )
        })?;
        let format = format.value();
        check_date_format(&format).map_err(|message| syn::Error::new(text.span(), message))?;
        return Ok(Some(BuiltinCast::DateTimeFormat(format)));
    }
    Ok(None)
}

/// Checks a chrono format string when the model compiles.
fn check_date_format(format: &str) -> Result<(), String> {
    use chrono::format::{Item, StrftimeItems};

    if format.trim().is_empty() {
        return Err("the date-time format cannot be empty".to_owned());
    }
    if StrftimeItems::new(format).any(|item| matches!(item, Item::Error)) {
        return Err(format!(
            "`{format}` is not a valid chrono format (see chrono::format::strftime)"
        ));
    }
    Ok(())
}

/// The cast's name in the model's metadata.
pub(crate) fn builtin_name(cast: &BuiltinCast) -> String {
    match cast {
        BuiltinCast::Decimal(scale) => format!("decimal({scale})"),
        BuiltinCast::DateTimeFormat(format) => format!("datetime_format({format})"),
        BuiltinCast::UnixTimestamp => "unix_timestamp".to_owned(),
        BuiltinCast::UnixTimestampMs => "unix_timestamp_ms".to_owned(),
        BuiltinCast::Encrypted => "encrypted".to_owned(),
        BuiltinCast::EncryptedJson => "encrypted_json".to_owned(),
        BuiltinCast::Hashed => "hashed".to_owned(),
        BuiltinCast::BinaryUuid => "binary_uuid".to_owned(),
        BuiltinCast::BinaryUlid => "binary_ulid".to_owned(),
        BuiltinCast::Vector => "vector".to_owned(),
    }
}

/// The marker type carrying a `datetime_format` field's format.
fn format_marker(model: &Model, column: &Column) -> Ident {
    format_ident!(
        "__NavreeDateFormat_{}_{}",
        model.ident,
        plain_name(&column.ident)
    )
}

/// The cast's name in the model's metadata, for any cast.
pub(crate) fn cast_name(cast: &Cast) -> String {
    match cast {
        Cast::Json => "json".to_owned(),
        Cast::StringEnum => "string_enum".to_owned(),
        Cast::IntEnum => "int_enum".to_owned(),
        Cast::Builtin(builtin) => builtin_name(builtin),
        Cast::Castable => "castable".to_owned(),
        Cast::Inbound(path) => format!("inbound({})", quote!(#path).to_string().replace(' ', "")),
        Cast::Path(path) => quote!(#path).to_string().replace(' ', ""),
    }
}

/// The type implementing `Cast<T>` for a column's field (`T` is the field's
/// type, or its inner type for an `Option`).
pub(crate) fn cast_type(model: &Model, column: &Column, cast: &Cast, krate: &TokenStream) -> TokenStream {
    let value_ty = column.inner.as_ref().unwrap_or(&column.ty);
    match cast {
        Cast::Json => quote!(#krate::cast::AsJson),
        Cast::StringEnum => quote!(#krate::cast::StringEnum),
        Cast::IntEnum => quote!(#krate::cast::IntEnum),
        Cast::Castable => quote!(<#value_ty as #krate::cast::Castable>::Cast),
        Cast::Inbound(path) => quote!(#krate::cast::Inbound<#path>),
        Cast::Path(path) => quote!(#path),
        Cast::Builtin(builtin) => match builtin {
            BuiltinCast::Decimal(scale) => {
                let scale = proc_macro2::Literal::u32_unsuffixed(*scale);
                quote!(#krate::cast::DecimalScale<#scale>)
            }
            BuiltinCast::DateTimeFormat(_) => {
                let marker = format_marker(model, column);
                quote!(#krate::cast::DateTimeFormat<#marker>)
            }
            BuiltinCast::UnixTimestamp => quote!(#krate::cast::UnixTimestamp),
            BuiltinCast::UnixTimestampMs => quote!(#krate::cast::UnixTimestampMs),
            BuiltinCast::Encrypted => quote!(#krate::cast::Encrypted),
            BuiltinCast::EncryptedJson => quote!(#krate::cast::EncryptedJson),
            BuiltinCast::Hashed => quote!(#krate::cast::Hashed),
            BuiltinCast::BinaryUuid => quote!(#krate::cast::BinaryUuid),
            BuiltinCast::BinaryUlid => quote!(#krate::cast::BinaryUlid),
            BuiltinCast::Vector => quote!(#krate::cast::Vector),
        },
    }
}

// -------------------------------------------------------------------------
// Serde
// -------------------------------------------------------------------------

/// Serde's `rename_all` rules.
#[derive(Clone, Copy)]
enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "lowercase" => RenameRule::Lower,
            "UPPERCASE" => RenameRule::Upper,
            "PascalCase" => RenameRule::Pascal,
            "camelCase" => RenameRule::Camel,
            "snake_case" => RenameRule::Snake,
            "SCREAMING_SNAKE_CASE" => RenameRule::ScreamingSnake,
            "kebab-case" => RenameRule::Kebab,
            "SCREAMING-KEBAB-CASE" => RenameRule::ScreamingKebab,
            _ => return None,
        })
    }

    /// The field name as serde writes it under this rule.
    fn apply(self, field: &str) -> String {
        match self {
            RenameRule::Lower | RenameRule::Snake => field.to_owned(),
            RenameRule::Upper | RenameRule::ScreamingSnake => field.to_ascii_uppercase(),
            RenameRule::Pascal => {
                let mut pascal = String::new();
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            RenameRule::Camel => {
                let pascal = RenameRule::Pascal.apply(field);
                let mut chars = pascal.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
                    None => pascal,
                }
            }
            RenameRule::Kebab => field.replace('_', "-"),
            RenameRule::ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }
}

/// What serde says about the struct.
#[derive(Default)]
pub(crate) struct SerdeContainer {
    rename_all: Option<RenameRule>,
}

/// What serde says about a field.
#[derive(Default)]
pub(crate) struct SerdeField {
    rename: Option<String>,
    skip: bool,
    flatten: bool,
    custom: bool,
}

impl SerdeField {
    /// Whether serde writes the field its own way (`serialize_with`,
    /// `with`, `flatten`).
    pub(crate) fn is_custom(&self) -> bool {
        self.custom
    }
}

/// Skips a serde key's value (serde reports its own errors).
fn skip_serde_value(meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        let _: syn::Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|inner| skip_serde_value(&inner))?;
    }
    Ok(())
}

/// `"name"` or `(serialize = "name", deserialize = "...")`: the serialize
/// name.
fn serialize_name(meta: &ParseNestedMeta<'_>) -> syn::Result<Option<String>> {
    if meta.input.peek(Token![=]) {
        let name: LitStr = meta.value()?.parse()?;
        return Ok(Some(name.value()));
    }
    let mut name = None;
    if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|inner| {
            if inner.path.is_ident("serialize") {
                let value: LitStr = inner.value()?.parse()?;
                name = Some(value.value());
                Ok(())
            } else {
                skip_serde_value(&inner)
            }
        })?;
    }
    Ok(name)
}

/// Reads the struct's `#[serde(..)]` attributes.
pub(crate) fn serde_container(attrs: &[Attribute]) -> SerdeContainer {
    let mut serde = SerdeContainer::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                if let Some(rule) = serialize_name(&meta)?.as_deref().and_then(RenameRule::parse) {
                    serde.rename_all = Some(rule);
                }
                Ok(())
            } else {
                skip_serde_value(&meta)
            }
        });
    }
    serde
}

/// Reads a field's `#[serde(..)]` attributes.
pub(crate) fn serde_field(attrs: &[Attribute]) -> SerdeField {
    let mut serde = SerdeField::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            let path = &meta.path;
            if path.is_ident("rename") {
                if let Some(name) = serialize_name(&meta)? {
                    serde.rename = Some(name);
                }
            } else if path.is_ident("skip") || path.is_ident("skip_serializing") {
                serde.skip = true;
            } else if path.is_ident("flatten") {
                serde.flatten = true;
                serde.custom = true;
            } else if path.is_ident("serialize_with") || path.is_ident("with") || path.is_ident("getter") {
                serde.custom = true;
                skip_serde_value(&meta)?;
            } else {
                skip_serde_value(&meta)?;
            }
            Ok(())
        });
    }
    serde
}

// -------------------------------------------------------------------------
// What the analysis keeps
// -------------------------------------------------------------------------

/// A field stored across several columns (`columns(..)`), as seen from one
/// of its columns.
#[derive(Clone)]
pub(crate) struct MultiPart {
    /// The field's type.
    pub(crate) field_ty: Type,
    /// `T` when the field is an `Option<T>`.
    pub(crate) field_inner: Option<Type>,
    /// The `MultiColumnCast`.
    pub(crate) cast: syn::Path,
    /// This column's position.
    pub(crate) index: usize,
    /// Every column of the field, in order.
    pub(crate) names: Vec<String>,
}

/// The attribute layer's view of a column's field.
#[derive(Clone, Default)]
pub(crate) struct FieldInfo {
    /// The field's key in serialized records (`None`: serde never writes
    /// it).
    pub(crate) serialized: Option<String>,
    /// Serde writes the field its own way.
    pub(crate) serde_custom: bool,
    /// The mutator.
    pub(crate) set_with: Option<syn::Path>,
    /// The OpenAPI schema type.
    pub(crate) schema: Option<Type>,
    /// The field is stored across several columns.
    pub(crate) multi: Option<MultiPart>,
}

/// The attribute layer's view of the model.
#[derive(Default)]
pub(crate) struct ModelAttrs {
    pub(crate) fillable: Vec<String>,
    pub(crate) guarded: Option<Vec<String>>,
    pub(crate) unguarded: bool,
    pub(crate) hidden: Vec<String>,
    pub(crate) visible: Vec<String>,
    pub(crate) appends: Vec<Ident>,
    pub(crate) encrypter: Option<LitStr>,
    pub(crate) serialize_dates_with: Option<syn::Path>,
    pub(crate) resource: Option<Type>,
    /// Fields that are not columns but serde writes, and which are hidden
    /// (`#[model(skip, hidden)]`).
    pub(crate) skipped_hidden: Vec<String>,
}

/// The key serde writes field `ident` under, if any.
pub(crate) fn serialized_key(
    ident: &Ident,
    container: &SerdeContainer,
    field: &SerdeField,
) -> Option<String> {
    if field.skip || field.flatten {
        return None;
    }
    Some(field.rename.clone().unwrap_or_else(|| {
        let name = plain_name(ident);
        container.rename_all.map_or(name.clone(), |rule| rule.apply(&name))
    }))
}

/// The column names a list name stands for: a field's columns, or a
/// column (with or without a JSON path).
fn resolve_columns(
    columns: &[Column],
    name: &Name,
    key: &str,
    errors: &mut Errors,
) -> Vec<String> {
    match name {
        Name::Field(ident) => {
            let field = plain_name(ident);
            let found: Vec<String> = columns
                .iter()
                .filter(|column| plain_name(&column.ident) == field)
                .map(|column| column.name.clone())
                .collect();
            if found.is_empty() {
                let known: Vec<String> = columns.iter().map(|c| plain_name(&c.ident)).collect();
                let known: Vec<&str> = known.iter().map(String::as_str).collect();
                let hint = suggestion(&field, &known)
                    .map(|candidate| format!(" (did you mean `{candidate}`?)"))
                    .unwrap_or_default();
                errors.push(syn::Error::new(
                    ident.span(),
                    format!("`{key}({field})` names no field of the model{hint}"),
                ));
            }
            found
        }
        Name::Text(text) => {
            let value = text.value();
            let column = value.split("->").next().unwrap_or_default().trim().to_owned();
            if !columns.iter().any(|candidate| candidate.name == column) {
                errors.push(syn::Error::new(
                    text.span(),
                    format!("`{key}(\"{value}\")`: the model has no column `{column}`"),
                ));
            }
            vec![value]
        }
    }
}

/// A name for the visibility lists: a field's name (checked) or any text.
fn resolve_output(
    fields: &[String],
    name: &Name,
    key: &str,
    errors: &mut Errors,
) -> String {
    match name {
        Name::Field(ident) => {
            let field = plain_name(ident);
            if !fields.contains(&field) {
                let known: Vec<&str> = fields.iter().map(String::as_str).collect();
                let hint = suggestion(&field, &known)
                    .map(|candidate| format!(" (did you mean `{candidate}`?)"))
                    .unwrap_or_default();
                errors.push(syn::Error::new(
                    ident.span(),
                    format!(
                        "`{key}({field})` names no field{hint}; write other names (relations, appended values, extra columns) as strings: {key}(\"{field}\")"
                    ),
                ));
            }
            field
        }
        Name::Text(text) => text.value(),
    }
}

/// Adds `name` to `list` unless it is there.
fn push_unique(list: &mut Vec<String>, name: String) {
    if !list.contains(&name) {
        list.push(name);
    }
}

/// Resolves and checks the attribute layer's lists.
pub(crate) fn analyse(
    container: ContainerAttrs,
    columns: &[Column],
    flags: &[(String, FieldAttrs)],
    skipped_hidden: Vec<String>,
    field_names: &[String],
    errors: &mut Errors,
) -> ModelAttrs {
    let mut attrs = ModelAttrs {
        unguarded: container.unguarded.is_some(),
        appends: container.appends,
        encrypter: container.encrypter,
        serialize_dates_with: container.serialize_dates_with,
        resource: container.resource,
        skipped_hidden,
        ..ModelAttrs::default()
    };
    if let Some((names, _)) = &container.fillable {
        for name in names {
            for column in resolve_columns(columns, name, "fillable", errors) {
                push_unique(&mut attrs.fillable, column);
            }
        }
    }
    if let Some((names, _)) = &container.guarded {
        let mut guarded = Vec::new();
        for name in names {
            for column in resolve_columns(columns, name, "guarded", errors) {
                push_unique(&mut guarded, column);
            }
        }
        attrs.guarded = Some(guarded);
    }
    for (column, field) in flags {
        if field.fillable.is_some() {
            push_unique(&mut attrs.fillable, column.clone());
        }
        if field.guarded.is_some() {
            push_unique(attrs.guarded.get_or_insert_with(Vec::new), column.clone());
        }
    }
    if let Some(span) = container.unguarded
        && (!attrs.fillable.is_empty() || attrs.guarded.is_some())
    {
        errors.push(syn::Error::new(
            span,
            "an unguarded model fills every column: drop `fillable` and `guarded`",
        ));
    }
    if let Some(guarded) = &attrs.guarded {
        for column in &attrs.fillable {
            if guarded.contains(column) {
                let span = container
                    .fillable
                    .as_ref()
                    .map_or_else(Span::call_site, |(_, span)| *span);
                errors.push(syn::Error::new(
                    span,
                    format!("`{column}` is both fillable and guarded: keep it in one list"),
                ));
            }
        }
    }
    for name in &container.hidden {
        let name = resolve_output(field_names, name, "hidden", errors);
        push_unique(&mut attrs.hidden, name);
    }
    for (column, field) in flags {
        if field.hidden.is_some() {
            let field_name = columns
                .iter()
                .find(|candidate| &candidate.name == column)
                .map_or_else(|| column.clone(), |found| plain_name(&found.ident));
            push_unique(&mut attrs.hidden, field_name);
        }
    }
    for name in attrs.skipped_hidden.clone() {
        push_unique(&mut attrs.hidden, name);
    }
    for name in &container.visible {
        let name = resolve_output(field_names, name, "visible", errors);
        push_unique(&mut attrs.visible, name);
    }
    attrs
}

// -------------------------------------------------------------------------
// Generated code
// -------------------------------------------------------------------------

fn string_list(names: &[String]) -> TokenStream {
    quote!(&[#(#names),*])
}

/// The `ModelMeta` builder calls of the attribute layer.
pub(crate) fn meta_calls(model: &Model, krate: &TokenStream) -> TokenStream {
    let attrs = &model.attributes;
    let mut calls = TokenStream::new();
    if !attrs.fillable.is_empty() {
        let list = string_list(&attrs.fillable);
        calls.extend(quote!(.fillable(#list)));
    }
    if let Some(guarded) = &attrs.guarded {
        let list = string_list(guarded);
        calls.extend(quote!(.guarded(#list)));
    }
    if attrs.unguarded {
        calls.extend(quote!(.unguarded()));
    }
    if !attrs.hidden.is_empty() {
        let list = string_list(&attrs.hidden);
        calls.extend(quote!(.hidden(#list)));
    }
    if !attrs.visible.is_empty() {
        let list = string_list(&attrs.visible);
        calls.extend(quote!(.visible(#list)));
    }
    if !attrs.appends.is_empty() {
        let names: Vec<String> = attrs.appends.iter().map(plain_name).collect();
        let list = string_list(&names);
        calls.extend(quote!(.appends(#list)));
    }
    if let Some(name) = &attrs.encrypter {
        calls.extend(quote!(.encrypter(#name)));
    }
    if let Some(path) = &attrs.serialize_dates_with {
        calls.extend(quote!(.serialize_dates_with(#path)));
    }
    let contexts: Vec<TokenStream> = model
        .columns
        .iter()
        .filter_map(|column| context_flag(model, column, krate))
        .collect();
    if !contexts.is_empty() {
        calls.extend(quote!(.cast_context(false #(|| #contexts)*)));
    }
    calls
}

/// `<C as Cast<T>>::CONTEXT` for a cast column (once per multi-column
/// field).
fn context_flag(model: &Model, column: &Column, krate: &TokenStream) -> Option<TokenStream> {
    if let Some(multi) = &column.field.multi {
        if multi.index != 0 {
            return None;
        }
        let cast = &multi.cast;
        let value_ty = multi.field_inner.as_ref().unwrap_or(&multi.field_ty);
        return Some(quote!(<#cast as #krate::cast::MultiColumnCast<#value_ty>>::CONTEXT));
    }
    let cast = column.cast.as_ref()?;
    let cast_ty = cast_type(model, column, cast, krate);
    let value_ty = column.inner.as_ref().unwrap_or(&column.ty);
    Some(quote!(<#cast_ty as #krate::cast::Cast<#value_ty>>::CONTEXT))
}

/// The `ColumnMeta` builder calls of the attribute layer.
pub(crate) fn column_meta_calls(model: &Model, column: &Column, krate: &TokenStream) -> TokenStream {
    let field = &column.field;
    let mut calls = TokenStream::new();
    let serialized = match &field.serialized {
        Some(key) => quote!(::core::option::Option::Some(#key)),
        None => quote!(::core::option::Option::None),
    };
    calls.extend(quote!(.serialized_as(#serialized)));
    let kind = if let Some(multi) = &field.multi {
        let cast = &multi.cast;
        let value_ty = multi.field_inner.as_ref().unwrap_or(&multi.field_ty);
        quote!(<#cast as #krate::cast::MultiColumnCast<#value_ty>>::KIND)
    } else if let Some(cast) = &column.cast {
        let cast_ty = cast_type(model, column, cast, krate);
        let value_ty = column.inner.as_ref().unwrap_or(&column.ty);
        quote!(<#cast_ty as #krate::cast::Cast<#value_ty>>::KIND)
    } else {
        let ty = &column.ty;
        quote!(<#ty as #krate::Field>::KIND)
    };
    calls.extend(quote!(.kind(#kind)));
    if field.serde_custom {
        calls.extend(quote!(.serde_custom()));
    }
    if field.set_with.is_some() {
        calls.extend(quote!(.mutator()));
    }
    if let Some(schema) = &field.schema {
        calls.extend(quote!(.schema(#krate::__private::schema_json::<#schema>)));
    }
    calls
}

/// The generated `Model` methods of the attribute layer.
pub(crate) fn trait_items(model: &Model, krate: &TokenStream) -> TokenStream {
    let private = quote!(#krate::__private);
    let mut set_arms = Vec::new();
    let mut changed = Vec::new();
    let mut json_arms = Vec::new();
    let mut prepares = Vec::new();
    let mut overrides = Vec::new();
    let mut stale = Vec::new();
    let mut increment_arms = Vec::new();
    let mut part_arms = Vec::new();

    for column in &model.columns {
        let ident = &column.ident;
        let name = &column.name;
        let field_name = plain_name(ident);
        let ty = &column.ty;

        if let Some(multi) = &column.field.multi {
            let cast = &multi.cast;
            let field_ty = &multi.field_ty;
            let value_ty = multi.field_inner.as_ref().unwrap_or(field_ty);
            let index = multi.index;
            let (replace, changes) = if multi.field_inner.is_some() {
                (quote!(replace_multi_option), quote!(changed_multi_option))
            } else {
                (quote!(replace_multi), quote!(changed_multi))
            };
            let assign = match &column.field.set_with {
                Some(mutator) => quote!(#mutator(self, value);),
                None => quote!(self.#ident = value;),
            };
            set_arms.push(quote! {
                #name => {
                    let part: #ty = #private::input::<Self, #ty>(column, input)?;
                    let value: #field_ty = #private::#replace::<Self, #cast, #value_ty>(
                        #field_name, &self.#ident, column, #index, #krate::Field::to_value(&part),
                    )?;
                    #assign
                    ::core::result::Result::Ok(())
                }
            });
            if index == 0 {
                let names = &multi.names;
                changed.push(quote! {
                    for index in #private::#changes::<Self, #cast, #value_ty>(#field_name, &self.#ident, &original.#ident) {
                        const NAMES: &[&str] = &[#(#names),*];
                        changed.push(NAMES[index]);
                    }
                });
                let parts: Vec<&Column> = model
                    .columns
                    .iter()
                    .filter(|part| part.ident == column.ident && part.field.multi.is_some())
                    .collect();
                let part_arms_of_field = parts.iter().enumerate().map(|(position, part)| {
                    let part_name = &part.name;
                    let part_ty = &part.ty;
                    quote! {
                        #part_name => {
                            let part: #part_ty = #private::input::<Self, #part_ty>(&column, input)?;
                            values[#position] = #krate::Field::to_value(&part);
                        }
                    }
                });
                let (current, from_values) = if multi.field_inner.is_some() {
                    (
                        quote!(#private::write_multi_option::<Self, #cast, #value_ty>(#field_name, &self.#ident)?),
                        quote!(multi_from_values_option),
                    )
                } else {
                    (
                        quote!(#private::multi_values::<Self, #cast, #value_ty>(#field_name, &self.#ident)?),
                        quote!(multi_from_values),
                    )
                };
                part_arms.push(quote! {
                    #field_name => {
                        let mut values: ::std::vec::Vec<#krate::Value> = #current;
                        for (column, input) in inputs {
                            match column.as_str() {
                                #(#part_arms_of_field)*
                                _ => {
                                    return ::core::result::Result::Err(#private::unknown_column::<Self>(&column));
                                }
                            }
                        }
                        let value: #field_ty = #private::#from_values::<Self, #cast, #value_ty>(
                            #field_name, &[#(#names),*], values,
                        )?;
                        #assign
                        ::core::result::Result::Ok(())
                    }
                });
            }
            continue;
        }

        // set_attribute: untyped input through the cast and the mutator.
        let value_ty = column.inner.as_ref().unwrap_or(ty);
        let convert = match &column.cast {
            None => quote!(#private::input::<Self, #ty>(column, input)?),
            Some(cast) => {
                let cast_ty = cast_type(model, column, cast, krate);
                if column.inner.is_some() {
                    quote!(#private::input_cast_option::<Self, #cast_ty, #value_ty>(column, input)?)
                } else {
                    quote!(#private::input_cast::<Self, #cast_ty, #value_ty>(column, input)?)
                }
            }
        };
        let assign = match &column.field.set_with {
            Some(mutator) => quote!(#mutator(self, value);),
            None => quote!(self.#ident = value;),
        };
        set_arms.push(quote! {
            #name => {
                let value: #ty = #convert;
                #assign
                ::core::result::Result::Ok(())
            }
        });

        // changed_columns.
        let same = match &column.cast {
            None => quote!(#private::same_field::<#ty>(&self.#ident, &original.#ident)),
            Some(cast) => {
                let cast_ty = cast_type(model, column, cast, krate);
                if column.inner.is_some() {
                    quote!(#private::same_cast_option::<#cast_ty, #value_ty>(&self.#ident, &original.#ident))
                } else {
                    quote!(#private::same_cast::<#cast_ty, #value_ty>(&self.#ident, &original.#ident))
                }
            }
        };
        changed.push(quote! {
            if !(#same) {
                changed.push(#name);
            }
        });

        // json_attribute.
        match &column.cast {
            None => json_arms.push(quote! {
                #name => (&#private::JsonProbe::<#ty>(&self.#ident)).json_attribute(),
            }),
            Some(Cast::Json | Cast::Builtin(BuiltinCast::EncryptedJson)) => {
                json_arms.push(quote! {
                    #name => #private::json_of::<Self, #ty>(column, &self.#ident).map(::core::option::Option::Some),
                });
            }
            Some(cast) => {
                let cast_ty = cast_type(model, column, cast, krate);
                let write = if column.inner.is_some() {
                    quote!(#private::write_cast_option::<Self, #cast_ty, #value_ty>(column, &self.#ident)?)
                } else {
                    quote!(#private::write_cast::<Self, #cast_ty, #value_ty>(column, &self.#ident)?)
                };
                json_arms.push(quote! {
                    #name => ::core::result::Result::Ok(#private::stored_json(#write)),
                });
            }
        }

        let Some(cast) = &column.cast else {
            continue;
        };
        let cast_ty = cast_type(model, column, cast, krate);
        let optional = column.inner.is_some();

        // prepare_for_write.
        prepares.push(if optional {
            quote!(#private::prepare_cast_option::<Self, #cast_ty, #value_ty>(#name, &mut self.#ident, context).await?;)
        } else {
            quote!(#private::prepare_cast::<Self, #cast_ty, #value_ty>(#name, &mut self.#ident, context).await?;)
        });

        // serialized_overrides.
        if let Some(key) = &column.field.serialized
            && !column.field.serde_custom
        {
            let serialize = if optional {
                quote!(#private::serialize_cast_option::<Self, #cast_ty, #value_ty>(#name, &self.#ident)?)
            } else {
                quote!(#private::serialize_cast::<Self, #cast_ty, #value_ty>(#name, &self.#ident)?)
            };
            overrides.push(quote! {
                if let ::core::option::Option::Some(json) = #serialize {
                    overrides.push((#key, json));
                }
            });
        }

        // stale_columns.
        stale.push(quote! {
            if <#cast_ty as #krate::cast::Cast<#value_ty>>::CONTEXT
                && #private::stale_cast::<#cast_ty, #value_ty>(row, #name)
            {
                stale.push(#name);
            }
        });

        // increment_cast.
        increment_arms.push(if optional {
            quote!(#name => #private::increment_cast_option::<Self, #cast_ty, #value_ty>(column, &mut self.#ident, amount, decrement),)
        } else {
            quote!(#name => #private::increment_cast::<Self, #cast_ty, #value_ty>(column, &mut self.#ident, amount, decrement),)
        });
    }

    let prepare_fn = if prepares.is_empty() {
        quote!()
    } else {
        quote! {
            fn prepare_for_write<'navree>(
                &'navree mut self,
                context: &'navree #private::CastContext,
            ) -> #private::BoxFuture<'navree, #krate::Result<()>> {
                ::std::boxed::Box::pin(async move {
                    #(#prepares)*
                    ::core::result::Result::Ok(())
                })
            }
        }
    };
    let overrides_fn = if overrides.is_empty() {
        quote!()
    } else {
        quote! {
            fn serialized_overrides(&self) -> #krate::Result<::std::vec::Vec<(&'static str, #private::serde_json::Value)>> {
                let mut overrides = ::std::vec::Vec::new();
                #(#overrides)*
                ::core::result::Result::Ok(overrides)
            }
        }
    };
    let stale_fn = if stale.is_empty() {
        quote!()
    } else {
        quote! {
            fn stale_columns(row: &#krate::Row) -> ::std::vec::Vec<&'static str> {
                let mut stale = ::std::vec::Vec::new();
                #(#stale)*
                stale
            }
        }
    };
    let parts_fn = if part_arms.is_empty() {
        quote!()
    } else {
        quote! {
            fn set_attribute_parts(
                &mut self,
                field: &str,
                inputs: ::std::vec::Vec<(::std::string::String, #private::serde_json::Value)>,
            ) -> #krate::Result<()> {
                match field {
                    #(#part_arms)*
                    _ => {
                        for (column, input) in inputs {
                            #krate::Model::set_attribute(self, &column, input)?;
                        }
                        ::core::result::Result::Ok(())
                    }
                }
            }
        }
    };
    let increment_fn = if increment_arms.is_empty() {
        quote!()
    } else {
        quote! {
            fn increment_cast(&mut self, column: &str, amount: #krate::Value, decrement: bool) -> #krate::Result<()> {
                match column {
                    #(#increment_arms)*
                    _ => ::core::result::Result::Err(#private::not_a_cast_column::<Self>(column)),
                }
            }
        }
    };

    quote! {
        fn set_attribute(&mut self, column: &str, input: #private::serde_json::Value) -> #krate::Result<()> {
            match column {
                #(#set_arms)*
                _ => ::core::result::Result::Err(#private::unknown_column::<Self>(column)),
            }
        }

        fn changed_columns(&self, original: &Self) -> #krate::Result<::std::vec::Vec<&'static str>> {
            let mut changed = ::std::vec::Vec::new();
            #(#changed)*
            ::core::result::Result::Ok(changed)
        }

        fn json_attribute(&self, column: &str) -> #krate::Result<::core::option::Option<#private::serde_json::Value>> {
            #[allow(unused_imports)]
            use #private::{JsonProbeAny as _, JsonProbeJson as _};
            match column {
                #(#json_arms)*
                _ => ::core::result::Result::Ok(::core::option::Option::None),
            }
        }

        fn accessors() -> &'static [#krate::attributes::Accessor<Self>] {
            #[allow(unused_imports)]
            use #private::NoAccessors as _;
            Self::__NAVREE_ACCESSORS
        }

        #prepare_fn
        #overrides_fn
        #stale_fn
        #increment_fn
        #parts_fn
    }
}

/// Setters of fields with mutators (`set_<field>`), and the names they
/// must not take.
pub(crate) fn inherent_items(model: &Model, errors: &mut Errors) -> TokenStream {
    /// Methods of `Record` and `Model` a setter would shadow.
    const RESERVED: &[&str] = &[
        "set_key",
        "set_value",
        "set_values",
        "set_attribute",
        "set_extra",
        "set_hidden",
        "set_visible",
        "set_appends",
        "set_json_path",
        "set_created_at",
        "set_updated_at",
    ];
    let vis = &model.vis;
    let mut setters = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for column in &model.columns {
        let Some(mutator) = &column.field.set_with else {
            continue;
        };
        let field_name = plain_name(&column.ident);
        if done.contains(&field_name) {
            continue;
        }
        done.push(field_name.clone());
        let setter = format_ident!("set_{field_name}", span = column.ident.span());
        if RESERVED.contains(&setter.to_string().as_str()) {
            errors.push(syn::Error::new(
                column.ident.span(),
                format!(
                    "the setter of field `{field_name}` would be `{setter}`, which records already have: rename the field (with #[model(column = \"{field_name}\")] to keep the column)"
                ),
            ));
            continue;
        }
        let ty = column
            .field
            .multi
            .as_ref()
            .map_or(&column.ty, |multi| &multi.field_ty);
        let doc = format!(
            "Sets `{field_name}` through its mutator (`{}`).",
            quote!(#mutator).to_string().replace(' ', "")
        );
        setters.push(quote! {
            #[doc = #doc]
            #vis fn #setter(&mut self, value: impl ::core::convert::Into<#ty>) {
                #mutator(self, ::core::convert::Into::into(value));
            }
        });
    }
    quote!(#(#setters)*)
}

/// Items beside the model: the formats of `datetime_format` casts, and the
/// compile-time check of `appends(..)`.
pub(crate) fn module_items(model: &Model, krate: &TokenStream) -> TokenStream {
    let mut items = Vec::new();
    for column in &model.columns {
        if let Some(Cast::Builtin(BuiltinCast::DateTimeFormat(format))) = &column.cast {
            let marker = format_marker(model, column);
            let doc = format!(
                "The format of `{}::{}` (`{format}`).",
                model.ident,
                plain_name(&column.ident)
            );
            items.push(quote! {
                #[doc = #doc]
                #[doc(hidden)]
                #[allow(non_camel_case_types)]
                struct #marker;

                impl #krate::cast::DateTimeFormatSpec for #marker {
                    const FORMAT: &'static str = #format;
                }
            });
        }
    }
    let ident = &model.ident;
    for append in &model.attributes.appends {
        let name = plain_name(append);
        let message = format!(
            "{ident} appends `{name}` (#[model(appends({name}))]), but it has no accessor named `{name}`: mark `fn {name}(&self) -> T` with #[accessor] in an #[accessors] impl block of {ident}"
        )
        .replace('{', "{{")
        .replace('}', "}}");
        items.push(quote_spanned! {append.span()=>
            const _: () = {
                #[allow(unused_imports)]
                use #krate::__private::NoAccessors as _;
                assert!(#krate::__private::has_accessor(#ident::__NAVREE_ACCESSORS, #name), #message);
            };
        });
    }
    quote!(#(#items)*)
}

/// The column constant's name for column `name` of a multi-column field.
pub(crate) fn part_constant(name: &str, span: Span) -> Ident {
    format_ident!("{}", name.to_shouty_snake_case(), span = span)
}
