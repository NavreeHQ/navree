//! `#[flag]`: implements `navree_flags::Flag` for a unit struct.

use heck::ToKebabCase;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{Expr, Fields, ItemStruct, Lit, LitBool, LitStr, Meta, Token};

use crate::attr::{Errors, key_name};
use crate::paths::FLAGS;

/// The longest flag name (navree-flags' `MAX_NAME_LEN`).
const MAX_NAME_LEN: usize = 100;

/// Expands `#[flag(args)] struct X;`. On a mistake, the item is emitted
/// unchanged next to the error, so nothing else cascades.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let parsed = match parse(args, item.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(item);
            return out;
        }
    };
    match FLAGS.path(Span::call_site()) {
        Ok(flags) => generate(&parsed, &flags),
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(parsed.item.into_token_stream());
            out
        }
    }
}

/// A parsed `#[flag]`.
pub(crate) struct ParsedFlag {
    item: ItemStruct,
    name: String,
    default: bool,
    expose: bool,
    description: String,
}

/// Parses the arguments and the struct.
pub(crate) fn parse(args: TokenStream, item: TokenStream) -> syn::Result<ParsedFlag> {
    let item: ItemStruct = syn::parse2(item).map_err(|error| {
        syn::Error::new(
            error.span(),
            "#[flag] goes on a unit struct: `pub struct NewPipeline;`",
        )
    })?;
    let mut errors = Errors::default();
    if !matches!(item.fields, Fields::Unit) {
        errors.push(syn::Error::new_spanned(
            &item.fields,
            format!(
                "a flag is a unit struct, without fields: `struct {};`",
                item.ident
            ),
        ));
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &item.generics,
            "a flag cannot be generic: its name is one stored key",
        ));
    }
    let args = errors.keep(Args::parse(args)).unwrap_or_default();
    let name = match &args.name {
        Some(name) => {
            errors.keep(check_name(&name.value()).map_err(|m| syn::Error::new(name.span(), m)));
            name.value()
        }
        None => item.ident.to_string().to_kebab_case(),
    };
    errors.finish()?;
    Ok(ParsedFlag {
        description: description(&item),
        name,
        default: args.default.as_ref().is_some_and(LitBool::value),
        expose: args.expose,
        item,
    })
}

/// The item plus `impl Flag`, naming navree-flags as `flags`.
pub(crate) fn generate(parsed: &ParsedFlag, flags: &TokenStream) -> TokenStream {
    let item = &parsed.item;
    let ident = &item.ident;
    let (name, default, expose, description) = (
        &parsed.name,
        parsed.default,
        parsed.expose,
        &parsed.description,
    );
    quote! {
        #item

        impl #flags::Flag for #ident {
            const NAME: &'static str = #name;
            const DEFAULT: bool = #default;
            const EXPOSE: bool = #expose;
            const DESCRIPTION: &'static str = #description;
        }
    }
}

/// `name = "..", default = true|false, expose [= true|false]`.
#[derive(Default)]
struct Args {
    name: Option<LitStr>,
    default: Option<LitBool>,
    expose: bool,
}

impl Args {
    fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut parsed = Args::default();
        let mut seen: Vec<String> = Vec::new();
        let parser = syn::meta::parser(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            match key.as_str() {
                "name" => parsed.name = Some(meta.value()?.parse()?),
                "default" => parsed.default = Some(meta.value()?.parse()?),
                "expose" => {
                    parsed.expose = if meta.input.peek(Token![=]) {
                        meta.value()?.parse::<LitBool>()?.value
                    } else {
                        true
                    };
                }
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[flag] key `{key}`: expected `name = \"..\"`, `default = true` \
                         or `expose`"
                    )));
                }
            }
            seen.push(key);
            Ok(())
        });
        syn::parse::Parser::parse2(parser, args)?;
        Ok(parsed)
    }
}

/// navree-flags' name rule: 1 to 100 characters, lower-case letters,
/// digits, `-`, `_` and `.`, starting with a letter or a digit.
fn check_name(name: &str) -> Result<(), String> {
    let valid =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let starts_well = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if name.is_empty() || name.len() > MAX_NAME_LEN || !starts_well || !name.chars().all(valid) {
        return Err(format!(
            "invalid flag name `{name}`: use 1 to {MAX_NAME_LEN} lower-case letters, digits, \
             `-`, `_` and `.`, starting with a letter or a digit (`new-pipeline`)"
        ));
    }
    Ok(())
}

/// The doc comment, its lines trimmed and joined with spaces.
fn description(item: &ItemStruct) -> String {
    let lines: Vec<String> = item
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(lit) => match &lit.lit {
                    Lit::Str(text) => Some(text.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .flat_map(|text| {
            text.lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|line| !line.is_empty())
        .collect();
    lines.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_with_facade(args: TokenStream, item: TokenStream) -> String {
        match parse(args, item) {
            Ok(parsed) => generate(&parsed, &quote!(::navree::flags)).to_string(),
            Err(error) => format!("error: {error}"),
        }
    }

    #[test]
    fn a_plain_flag_takes_its_name_from_the_type() {
        let out = expand_with_facade(
            quote!(),
            quote! {
                /// The redesigned deal pipeline board
                /// with drag-and-drop between stages.
                pub struct NewPipeline;
            },
        );
        assert!(
            out.contains("impl :: navree :: flags :: Flag for NewPipeline"),
            "{out}"
        );
        assert!(
            out.contains("const NAME : & 'static str = \"new-pipeline\""),
            "{out}"
        );
        assert!(out.contains("const DEFAULT : bool = false"), "{out}");
        assert!(out.contains("const EXPOSE : bool = false"), "{out}");
        assert!(
            out.contains(
                "\"The redesigned deal pipeline board with drag-and-drop between stages.\""
            ),
            "{out}"
        );
        assert!(
            out.contains("pub struct NewPipeline ;"),
            "the item is kept: {out}"
        );
    }

    #[test]
    fn arguments_set_the_name_default_and_exposure() {
        let out = expand_with_facade(
            quote!(name = "ai-summaries", default = true, expose),
            quote!(
                pub struct AiSummaries;
            ),
        );
        assert!(out.contains("\"ai-summaries\""), "{out}");
        assert!(out.contains("const DEFAULT : bool = true"), "{out}");
        assert!(out.contains("const EXPOSE : bool = true"), "{out}");
        assert!(
            out.contains("const DESCRIPTION : & 'static str = \"\""),
            "{out}"
        );

        let hidden = expand_with_facade(
            quote!(expose = false),
            quote!(
                struct HttpCache;
            ),
        );
        assert!(hidden.contains("\"http-cache\""), "{hidden}");
        assert!(hidden.contains("const EXPOSE : bool = false"), "{hidden}");
    }

    #[test]
    fn mistakes_are_reported() {
        let cases = [
            (
                quote!(),
                quote!(
                    pub struct Board {
                        on: bool,
                    }
                ),
                "a flag is a unit struct",
            ),
            (
                quote!(),
                quote!(
                    pub enum Board {
                        A,
                    }
                ),
                "#[flag] goes on a unit struct",
            ),
            (
                quote!(),
                quote!(
                    pub struct Board<T>;
                ),
                "cannot be generic",
            ),
            (
                quote!(label = "x"),
                quote!(
                    struct Board;
                ),
                "unknown #[flag] key `label`",
            ),
            (
                quote!(name = "Board V2"),
                quote!(
                    struct Board;
                ),
                "invalid flag name `Board V2`",
            ),
            (
                quote!(name = ""),
                quote!(
                    struct Board;
                ),
                "invalid flag name ``",
            ),
            (
                quote!(default = "yes"),
                quote!(
                    struct Board;
                ),
                "expected boolean literal",
            ),
            (
                quote!(expose, expose),
                quote!(
                    struct Board;
                ),
                "duplicate `expose` key",
            ),
        ];
        for (args, item, expected) in cases {
            let out = expand_with_facade(args, item);
            assert!(out.contains(expected), "expected `{expected}` in: {out}");
        }
    }
}
