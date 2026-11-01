//! What the derives share: the runtime crate's path, names as serde reads them,
//! doc comments and field types.

use heck::{ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase};
use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Fields, FieldsNamed, GenericArgument, LitStr, PathArguments, Type};

/// The path to navree-ui as the crate being compiled sees it.
pub(crate) fn ui_path(span: Span) -> syn::Result<TokenStream> {
    if let Ok(found) = proc_macro_crate::crate_name("navree-ui") {
        return Ok(match found {
            FoundCrate::Itself => quote!(crate),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name)
            }
        });
    }
    if let Ok(found) = proc_macro_crate::crate_name("navree") {
        return Ok(match found {
            FoundCrate::Itself => quote!(crate::ui),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name::ui)
            }
        });
    }
    Err(syn::Error::new(span, "this derive needs the `navree` crate (or `navree-ui`) in the [dependencies] of your Cargo.toml"))
}

/// The named fields of a struct.
pub(crate) fn named_fields<'a>(input: &'a DeriveInput, derive: &str) -> syn::Result<&'a FieldsNamed> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(fields),
            _ => Err(syn::Error::new_spanned(&input.ident, format!("`#[derive({derive})]` needs named fields"))),
        },
        _ => Err(syn::Error::new_spanned(&input.ident, format!("`#[derive({derive})]` works on structs"))),
    }
}

/// The first line of the doc comment.
pub(crate) fn doc_line(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().filter(|attr| attr.path().is_ident("doc")).find_map(|attr| {
        let syn::Meta::NameValue(meta) = &attr.meta else { return None };
        let syn::Expr::Lit(lit) = &meta.value else { return None };
        let syn::Lit::Str(text) = &lit.lit else { return None };
        let line = text.value();
        let line = line.trim();
        (!line.is_empty()).then(|| line.trim_end_matches('.').to_owned())
    })
}

/// A name as words: `expected_close_date` is `Expected close date`.
pub(crate) fn words(name: &str) -> String {
    if name.len() > 1 && !name.contains('_') && name.chars().all(|c| !c.is_lowercase()) {
        return name.to_owned();
    }
    let title = name.to_title_case().to_lowercase();
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `rename = "..."` in `#[serde(...)]`.
pub(crate) fn serde_rename(attrs: &[Attribute]) -> Option<String> {
    serde_key(attrs, "rename")
}

/// `rename_all = "..."` in `#[serde(...)]`.
pub(crate) fn serde_rename_all(attrs: &[Attribute]) -> Option<String> {
    serde_key(attrs, "rename_all")
}

fn serde_key(attrs: &[Attribute], key: &str) -> Option<String> {
    let mut found = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(key) {
                if meta.input.peek(syn::Token![=]) {
                    let value: LitStr = meta.value()?.parse()?;
                    found = Some(value.value());
                }
            } else if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: TokenStream = content.parse()?;
            }
            Ok(())
        });
    }
    found
}

/// `name` in the case serde's `rename_all` names.
pub(crate) fn apply_case(name: &str, case: Option<&str>) -> String {
    match case {
        Some("lowercase") => name.replace('_', "").to_lowercase(),
        Some("UPPERCASE") => name.replace('_', "").to_uppercase(),
        Some("PascalCase") => name.to_upper_camel_case(),
        Some("camelCase") => name.to_lower_camel_case(),
        Some("snake_case") => name.to_snake_case(),
        Some("SCREAMING_SNAKE_CASE") => name.to_shouty_snake_case(),
        Some("kebab-case") => name.to_kebab_case(),
        Some("SCREAMING-KEBAB-CASE") => name.to_shouty_kebab_case(),
        _ => name.to_owned(),
    }
}

/// The type inside `Option<...>`, when it is one.
pub(crate) fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else { return None };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

/// The last segment of a type's path: `chrono::NaiveDate` is `NaiveDate`.
pub(crate) fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else { return None };
    Some(path.path.segments.last()?.ident.to_string())
}
