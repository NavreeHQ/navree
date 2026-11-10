//! Field names as serde reads them, so a rule's path is the input's key:
//! `#[serde(rename = "...")]`, `#[serde(rename_all = "...")]` and
//! `#[serde(flatten)]`. Anything else in `#[serde(...)]` is serde's
//! business and ignored here (serde reports its own mistakes).

use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use syn::meta::ParseNestedMeta;
use syn::{Attribute, LitStr};

/// What serde says about a field.
#[derive(Default)]
pub(crate) struct SerdeField {
    /// `rename = "..."` (or its `deserialize` half).
    pub(crate) rename: Option<String>,
    /// `flatten`: the field's own fields sit next to its siblings.
    pub(crate) flatten: bool,
}

/// `rename_all = "..."` (or its `deserialize` half) on a container or an
/// enum variant.
pub(crate) fn rename_all(attrs: &[Attribute]) -> Option<String> {
    let mut found = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                found = deserialize_name(&meta)?.or(found.take());
            } else {
                skip(&meta)?;
            }
            Ok(())
        });
    }
    found
}

/// The serde options of a field.
pub(crate) fn field(attrs: &[Attribute]) -> SerdeField {
    let mut field = SerdeField::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                if let Some(name) = deserialize_name(&meta)? {
                    field.rename = Some(name);
                }
            } else if meta.path.is_ident("flatten") {
                field.flatten = true;
            } else {
                skip(&meta)?;
            }
            Ok(())
        });
    }
    field
}

/// `key = "name"` or `key(deserialize = "name", serialize = "...")`.
fn deserialize_name(meta: &ParseNestedMeta) -> syn::Result<Option<String>> {
    if meta.input.peek(syn::Token![=]) {
        let name: LitStr = meta.value()?.parse()?;
        return Ok(Some(name.value()));
    }
    let mut name = None;
    meta.parse_nested_meta(|inner| {
        let value: LitStr = inner.value()?.parse()?;
        if inner.path.is_ident("deserialize") {
            name = Some(value.value());
        }
        Ok(())
    })?;
    Ok(name)
}

/// Consumes an option this macro doesn't read.
fn skip(meta: &ParseNestedMeta) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        let _content;
        syn::parenthesized!(_content in meta.input);
    }
    Ok(())
}

/// `name` in the case `rename_all` names (`camelCase`, `kebab-case`, ...);
/// an unknown case leaves it as it is.
pub(crate) fn apply_case(name: &str, case: &str) -> String {
    match case {
        "lowercase" => name.to_lowercase(),
        "UPPERCASE" => name.to_uppercase(),
        "PascalCase" => name.to_upper_camel_case(),
        "camelCase" => name.to_lower_camel_case(),
        "snake_case" => name.to_snake_case(),
        "SCREAMING_SNAKE_CASE" => name.to_shouty_snake_case(),
        "kebab-case" => name.to_kebab_case(),
        "SCREAMING-KEBAB-CASE" => name.to_shouty_kebab_case(),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    #[test]
    fn renames_are_read() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[serde(default, rename = "firstName", skip_serializing_if = "Option::is_none")]),
            parse_quote!(#[doc = "x"]),
        ];
        assert_eq!(field(&attrs).rename.as_deref(), Some("firstName"));
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[serde(rename(serialize = "a", deserialize = "b"))])];
        assert_eq!(field(&attrs).rename.as_deref(), Some("b"));
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(flatten)])];
        assert!(field(&attrs).flatten);
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[serde(deny_unknown_fields, rename_all = "camelCase")])];
        assert_eq!(rename_all(&attrs).as_deref(), Some("camelCase"));
        assert_eq!(apply_case("first_name", "camelCase"), "firstName");
        assert_eq!(apply_case("first_name", "kebab-case"), "first-name");
        assert_eq!(
            apply_case("first_name", "SCREAMING_SNAKE_CASE"),
            "FIRST_NAME"
        );
        assert_eq!(apply_case("first_name", "weird"), "first_name");
    }
}
