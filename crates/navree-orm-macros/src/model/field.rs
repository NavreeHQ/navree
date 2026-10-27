//! The field attributes: `#[model(column = "...", cast = "...", default,
//! skip)]` on a field, plus the attribute layer's keys (see
//! [`attributes`](super::attributes)).

use proc_macro2::Span;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Attribute, LitStr, Token};

use super::attributes::{self, BuiltinCast};
use crate::errors::{Errors, key_name, suggestion};

/// The cast a field is stored through.
#[derive(Clone)]
pub(crate) enum Cast {
    /// `"json"`: any serde type, stored as JSON.
    Json,
    /// `"string_enum"`: a unit enum stored as its serde name.
    StringEnum,
    /// `"int_enum"`: an enum stored as an integer.
    IntEnum,
    /// A built-in of the attribute layer (`"decimal(2)"`, `"encrypted"`,
    /// ...).
    Builtin(BuiltinCast),
    /// `cast` without a value: the field type's own cast (`Castable`).
    Castable,
    /// `inbound = path`: an inbound cast.
    Inbound(syn::Path),
    /// A type implementing `Cast<T>` for the field's type (with any
    /// arguments given as `cast(Path, args..)`).
    Path(syn::Path),
}

/// Everything `#[model(...)]` on a field says.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    pub(crate) column: Option<LitStr>,
    pub(crate) cast: Option<(Cast, Span)>,
    pub(crate) default: bool,
    pub(crate) skip: Option<Span>,
    /// The attribute layer's keys.
    pub(crate) attributes: attributes::FieldAttrs,
}

/// The keys `#[model(...)]` takes on a field (the attribute layer's come
/// from [`attributes::FIELD_KEYS`]).
const KEYS: &[&str] = &["column", "cast", "default", "skip"];

/// Every field key, for messages.
fn all_keys() -> Vec<&'static str> {
    KEYS.iter().chain(attributes::FIELD_KEYS).copied().collect()
}

/// Parses the `#[model(...)]` attributes of a field.
pub(crate) fn parse(attrs: &[Attribute], errors: &mut Errors) -> FieldAttrs {
    let mut field = FieldAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        errors.keep(attr.parse_nested_meta(|meta| parse_key(&mut field, &meta)));
    }
    if let Some(inbound) = field.attributes.inbound.clone() {
        match &field.cast {
            Some((_, span)) => errors.push(syn::Error::new(
                *span,
                "a field takes `cast` or `inbound`, not both",
            )),
            None => {
                let span = inbound
                    .segments
                    .last()
                    .map_or_else(Span::call_site, |segment| segment.ident.span());
                field.cast = Some((Cast::Inbound(inbound), span));
            }
        }
    }
    field
}

fn parse_key(field: &mut FieldAttrs, meta: &ParseNestedMeta<'_>) -> syn::Result<()> {
    let path = &meta.path;
    if path.is_ident("column") {
        let column: LitStr = meta.value()?.parse()?;
        if column.value().trim().is_empty() {
            return Err(syn::Error::new(
                column.span(),
                "the column name cannot be empty",
            ));
        }
        field.column = Some(column);
    } else if path.is_ident("cast") {
        field.cast = Some(parse_cast(meta)?);
    } else if path.is_ident("default") {
        field.default = true;
    } else if path.is_ident("skip") {
        field.skip = Some(
            path.get_ident()
                .map_or_else(Span::call_site, syn::Ident::span),
        );
    } else if attributes::parse_field_key(&mut field.attributes, meta)? {
        // An attribute-layer key.
    } else {
        let key = key_name(path);
        let keys = all_keys();
        let hint = suggestion(&key, &keys)
            .map(|known| format!(" (did you mean `{known}`?)"))
            .unwrap_or_default();
        return Err(meta.error(format!(
            "unknown field attribute `{key}`{hint}; expected one of: {}",
            keys.join(", ")
        )));
    }
    Ok(())
}

/// `cast = "json"`, `cast = path::To::Cast`, `cast` (the type's own cast),
/// or `cast(path::To::Cast, args..)`.
fn parse_cast(meta: &ParseNestedMeta<'_>) -> syn::Result<(Cast, Span)> {
    let key_span = meta
        .path
        .get_ident()
        .map_or_else(Span::call_site, syn::Ident::span);
    if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let mut path: syn::Path = content.parse()?;
        let mut arguments = Punctuated::<syn::GenericArgument, Token![,]>::new();
        if content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
            arguments = Punctuated::parse_terminated(&content)?;
        }
        if arguments.is_empty() {
            return Err(syn::Error::new(
                key_span,
                "cast(..) takes the cast and its arguments, e.g. cast(path::To::Cast, 2); write `cast = path::To::Cast` for a cast without arguments",
            ));
        }
        let span = path
            .segments
            .last()
            .map_or(key_span, |segment| segment.ident.span());
        if let Some(last) = path.segments.last_mut() {
            if !last.arguments.is_empty() {
                return Err(syn::Error::new(
                    span,
                    "give the cast's arguments once: either cast(path::To::Cast, args..) or cast = path::To::Cast<args..>",
                ));
            }
            last.arguments = syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
                colon2_token: None,
                lt_token: Token![<](span),
                args: arguments,
                gt_token: Token![>](span),
            });
        }
        return Ok((Cast::Path(path), span));
    }
    if !meta.input.peek(Token![=]) {
        return Ok((Cast::Castable, key_span));
    }
    let value = meta.value()?;
    if value.peek(LitStr) {
        let text: LitStr = value.parse()?;
        let cast = match text.value().trim() {
            "json" => Cast::Json,
            "string_enum" => Cast::StringEnum,
            "int_enum" => Cast::IntEnum,
            other => {
                if let Some(builtin) = attributes::parse_builtin_cast(&text)? {
                    Cast::Builtin(builtin)
                } else {
                    let unknown = || {
                        syn::Error::new(
                            text.span(),
                            format!(
                                "unknown cast `{other}`: use {}, or the path of a type implementing `Cast`",
                                attributes::BUILTIN_CAST_NAMES
                            ),
                        )
                    };
                    let path = syn::parse_str::<syn::Path>(other).map_err(|_| unknown())?;
                    // A cast is a type: its name starts with a capital
                    // letter; a lowercase word is a misspelled built-in.
                    let is_type = path.segments.last().is_some_and(|segment| {
                        segment
                            .ident
                            .to_string()
                            .starts_with(|c: char| c.is_ascii_uppercase())
                    });
                    if !is_type {
                        return Err(unknown());
                    }
                    Cast::Path(path)
                }
            }
        };
        Ok((cast, text.span()))
    } else {
        let cast: syn::Path = value.parse()?;
        let span = cast
            .segments
            .last()
            .map_or_else(Span::call_site, |s| s.ident.span());
        Ok((Cast::Path(cast), span))
    }
}
