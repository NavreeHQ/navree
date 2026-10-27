//! The field attributes: `#[model(column = "...", cast = "...", default,
//! skip)]` on a field.

use proc_macro2::Span;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, LitStr};

use crate::errors::{Errors, key_name, suggestion};

/// The cast a field is stored through.
#[derive(Clone, Debug)]
pub(crate) enum Cast {
    /// `"json"`: any serde type, stored as JSON.
    Json,
    /// `"string_enum"`: a unit enum stored as its serde name.
    StringEnum,
    /// `"int_enum"`: an enum stored as an integer.
    IntEnum,
    /// A type implementing `Cast<T>` for the field's type.
    Path(syn::Path),
}

/// Everything `#[model(...)]` on a field says.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    pub(crate) column: Option<LitStr>,
    pub(crate) cast: Option<(Cast, Span)>,
    pub(crate) default: bool,
    pub(crate) skip: Option<Span>,
}

/// The keys `#[model(...)]` takes on a field.
const KEYS: &[&str] = &["column", "cast", "default", "skip"];

/// Parses the `#[model(...)]` attributes of a field.
pub(crate) fn parse(attrs: &[Attribute], errors: &mut Errors) -> FieldAttrs {
    let mut field = FieldAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("model")) {
        errors.keep(attr.parse_nested_meta(|meta| parse_key(&mut field, &meta)));
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
        let value = meta.value()?;
        let cast = if value.peek(LitStr) {
            let text: LitStr = value.parse()?;
            let cast = match text.value().as_str() {
                "json" => Cast::Json,
                "string_enum" => Cast::StringEnum,
                "int_enum" => Cast::IntEnum,
                other => {
                    let unknown = || {
                        syn::Error::new(
                            text.span(),
                            format!(
                                "unknown cast `{other}`: use \"json\", \"string_enum\", \"int_enum\" or the path of a type implementing `Cast`"
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
            };
            (cast, text.span())
        } else {
            let cast: syn::Path = value.parse()?;
            let span = cast
                .segments
                .last()
                .map_or_else(Span::call_site, |s| s.ident.span());
            (Cast::Path(cast), span)
        };
        field.cast = Some(cast);
    } else if path.is_ident("default") {
        field.default = true;
    } else if path.is_ident("skip") {
        field.skip = Some(
            path.get_ident()
                .map_or_else(Span::call_site, syn::Ident::span),
        );
    } else {
        let key = key_name(path);
        let hint = suggestion(&key, KEYS)
            .map(|known| format!(" (did you mean `{known}`?)"))
            .unwrap_or_default();
        return Err(meta.error(format!(
            "unknown field attribute `{key}`{hint}; expected one of: {}",
            KEYS.join(", ")
        )));
    }
    Ok(())
}
