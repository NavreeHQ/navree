//! `#[derive(Settings)]`: implements `navree_settings::SettingsGroup` for a
//! struct whose fields are the settings.

use heck::ToKebabCase;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, Fields, Ident, Lit, LitStr, Meta, Token};

use crate::attr::{Errors, key_name};
use crate::paths::SETTINGS;

/// The longest settings key (navree-settings' `MAX_KEY_LEN`).
const MAX_KEY_LEN: usize = 100;

/// The levels, in navree-settings' `ScopeKind` order.
const LEVELS: [(&str, &str); 4] = [
    ("global", "Global"),
    ("organization", "Organization"),
    ("workspace", "Workspace"),
    ("user", "User"),
];

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let parsed = match parse(input) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_compile_error(),
    };
    match SETTINGS.path(Span::call_site()) {
        Ok(settings) => generate(&parsed, &settings),
        Err(error) => error.into_compile_error(),
    }
}

/// A parsed `#[derive(Settings)]`.
pub(crate) struct ParsedSettings {
    ident: Ident,
    key: String,
    /// The `ScopeKind` variants listed, in chain order; `None` for the
    /// trait's default.
    scopes: Option<Vec<&'static str>>,
    description: String,
    permission: Option<LitStr>,
    read_permission: Option<LitStr>,
}

/// The struct-level `#[settings(..)]` arguments.
#[derive(Default)]
struct Args {
    key: Option<LitStr>,
    scopes: Option<(Span, Vec<(String, Span)>)>,
    permission: Option<LitStr>,
    read_permission: Option<LitStr>,
}

/// Parses the struct and its `#[settings(..)]` attribute.
pub(crate) fn parse(input: TokenStream) -> syn::Result<ParsedSettings> {
    let input: DeriveInput = syn::parse2(input)?;
    let mut errors = Errors::default();
    match &input.data {
        Data::Struct(data) => {
            if !matches!(data.fields, Fields::Named(_)) {
                errors.push(syn::Error::new(
                    data.fields.span(),
                    "#[derive(Settings)] needs a struct with named fields: each field is a setting",
                ));
            }
        }
        _ => errors.push(syn::Error::new(
            input.ident.span(),
            "#[derive(Settings)] goes on a struct whose fields are the settings",
        )),
    }
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        errors.push(syn::Error::new_spanned(
            &input.generics,
            "settings cannot be generic: their key is one stored key",
        ));
    }
    let args = errors.keep(parse_args(&input)).unwrap_or_default();
    let key = match &args.key {
        Some(key) => {
            errors.keep(check_key(&key.value()).map_err(|m| syn::Error::new(key.span(), m)));
            key.value()
        }
        None => match default_key(&input.ident.to_string()) {
            Some(key) => key,
            None => {
                errors.push(syn::Error::new(
                    input.ident.span(),
                    "name the key: #[settings(key = \"..\")] (the type name gives none)",
                ));
                String::new()
            }
        },
    };
    let scopes = match &args.scopes {
        None => None,
        Some((span, listed)) => {
            if listed.is_empty() {
                errors.push(syn::Error::new(
                    *span,
                    "list at least one level: scopes(global, organization, workspace, user)",
                ));
            }
            let mut variants = Vec::new();
            for (level, variant) in LEVELS {
                if listed.iter().any(|(name, _)| name == level) {
                    variants.push(variant);
                }
            }
            for (name, span) in listed {
                if !LEVELS.iter().any(|(level, _)| level == name) {
                    errors.push(syn::Error::new(
                        *span,
                        format!(
                            "unknown level `{name}`: expected `global`, `organization`, \
                             `workspace` or `user`"
                        ),
                    ));
                } else if listed.iter().filter(|(other, _)| other == name).count() > 1 {
                    errors.push(syn::Error::new(*span, format!("duplicate level `{name}`")));
                }
            }
            Some(variants)
        }
    };
    for permission in [&args.permission, &args.read_permission]
        .into_iter()
        .flatten()
    {
        if permission.value().trim().is_empty() {
            errors.push(syn::Error::new(permission.span(), "must not be empty"));
        }
    }
    errors.finish()?;
    Ok(ParsedSettings {
        description: description(&input),
        ident: input.ident,
        key,
        scopes,
        permission: args.permission,
        read_permission: args.read_permission,
    })
}

/// `impl SettingsGroup`, naming navree-settings as `settings`.
pub(crate) fn generate(parsed: &ParsedSettings, settings: &TokenStream) -> TokenStream {
    let ident = &parsed.ident;
    let (key, description) = (&parsed.key, &parsed.description);
    let scopes = parsed.scopes.as_ref().map(|variants| {
        let variants = variants.iter().map(|variant| format_ident!("{variant}"));
        quote! {
            const SCOPES: &'static [#settings::ScopeKind] = &[#(#settings::ScopeKind::#variants),*];
        }
    });
    let option = |value: &Option<LitStr>| match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    };
    let (permission, read_permission) =
        (option(&parsed.permission), option(&parsed.read_permission));
    quote! {
        #[automatically_derived]
        impl #settings::SettingsGroup for #ident {
            const KEY: &'static str = #key;
            #scopes
            const DESCRIPTION: &'static str = #description;
            const PERMISSION: ::core::option::Option<&'static str> = #permission;
            const READ_PERMISSION: ::core::option::Option<&'static str> = #read_permission;
        }
    }
}

fn parse_args(input: &DeriveInput) -> syn::Result<Args> {
    let mut args = Args::default();
    let mut seen: Vec<String> = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("settings") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            match key.as_str() {
                "key" => args.key = Some(meta.value()?.parse()?),
                "permission" => args.permission = Some(meta.value()?.parse()?),
                "read_permission" => args.read_permission = Some(meta.value()?.parse()?),
                "scopes" => {
                    let span = meta.path.span();
                    let mut listed = Vec::new();
                    // `scopes(a, b)`; a bare or empty `scopes` lists nothing
                    // and is reported below.
                    if meta.input.peek(syn::token::Paren) {
                        let content;
                        syn::parenthesized!(content in meta.input);
                        let names = Punctuated::<Ident, Token![,]>::parse_terminated_with(
                            &content,
                            Ident::parse_any,
                        )?;
                        listed.extend(names.iter().map(|name| (name.to_string(), name.span())));
                    }
                    args.scopes = Some((span, listed));
                }
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[settings] key `{key}`: expected `key = \"..\"`, \
                         `scopes(..)`, `permission = \"..\"` or `read_permission = \"..\"`"
                    )));
                }
            }
            seen.push(key);
            Ok(())
        })?;
    }
    Ok(args)
}

/// The key of a type without `key = ".."`: its name without a `Settings`
/// suffix, in kebab case (`TeamPortalSettings` → `team-portal`).
fn default_key(type_name: &str) -> Option<String> {
    let base = type_name.strip_suffix("Settings").unwrap_or(type_name);
    let key = base.to_kebab_case();
    (!key.is_empty() && check_key(&key).is_ok()).then_some(key)
}

/// navree-settings' key rule: 1 to 100 characters, lower-case letters,
/// digits, `-`, `_` and `.`, starting with a letter or a digit.
fn check_key(key: &str) -> Result<(), String> {
    let valid =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.');
    let starts_well = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if key.is_empty() || key.len() > MAX_KEY_LEN || !starts_well || !key.chars().all(valid) {
        return Err(format!(
            "invalid settings key `{key}`: use 1 to {MAX_KEY_LEN} lower-case letters, digits, \
             `-`, `_` and `.`, starting with a letter or a digit (`portal`)"
        ));
    }
    Ok(())
}

/// The doc comment, its lines trimmed and joined with spaces.
fn description(input: &DeriveInput) -> String {
    let lines: Vec<String> = input
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

    fn expand_with_facade(input: TokenStream) -> String {
        match parse(input) {
            Ok(parsed) => generate(&parsed, &quote!(::navree::settings)).to_string(),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn expands_the_documented_example() {
        let out = expand_with_facade(quote! {
            /// What the member portal shows.
            #[derive(Serialize)]
            #[settings(key = "portal", scopes(workspace, global, organization), permission = "portal.manage")]
            pub struct PortalSettings {
                pub show_leaderboard: bool,
            }
        });
        let expected = quote! {
            #[automatically_derived]
            impl ::navree::settings::SettingsGroup for PortalSettings {
                const KEY: &'static str = "portal";
                const SCOPES: &'static [::navree::settings::ScopeKind] = &[
                    ::navree::settings::ScopeKind::Global,
                    ::navree::settings::ScopeKind::Organization,
                    ::navree::settings::ScopeKind::Workspace
                ];
                const DESCRIPTION: &'static str = "What the member portal shows.";
                const PERMISSION: ::core::option::Option<&'static str> =
                    ::core::option::Option::Some("portal.manage");
                const READ_PERMISSION: ::core::option::Option<&'static str> =
                    ::core::option::Option::None;
            }
        };
        assert_eq!(out, expected.to_string());
    }

    #[test]
    fn the_key_defaults_to_the_type_name_and_the_levels_to_the_trait() {
        let out = expand_with_facade(quote! {
            pub struct TeamPortalSettings { pub visible: bool }
        });
        assert!(
            out.contains("const KEY : & 'static str = \"team-portal\""),
            "{out}"
        );
        assert!(!out.contains("SCOPES"), "{out}");
        assert!(
            out.contains("const DESCRIPTION : & 'static str = \"\""),
            "{out}"
        );
        assert_eq!(
            default_key("Notifications").as_deref(),
            Some("notifications")
        );
        assert_eq!(default_key("Settings"), None);
    }

    #[test]
    fn misuse_is_reported() {
        for (input, message) in [
            (
                quote!(
                    #[settings(key = "Portal")]
                    struct P {
                        a: bool,
                    }
                ),
                "invalid settings key `Portal`",
            ),
            (
                quote!(
                    #[settings(scopes(team))]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "unknown level `team`",
            ),
            (
                quote!(
                    #[settings(scopes())]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "list at least one level",
            ),
            (
                quote!(
                    #[settings(scopes(user, user))]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "duplicate level `user`",
            ),
            (
                quote!(
                    #[settings(colour = "red")]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "unknown #[settings] key `colour`",
            ),
            (
                quote!(
                    #[settings(key = "a", key = "b")]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "duplicate `key` key",
            ),
            (
                quote!(
                    struct PortalSettings(bool);
                ),
                "named fields",
            ),
            (
                quote!(
                    enum PortalSettings {
                        A,
                    }
                ),
                "goes on a struct",
            ),
            (
                quote!(
                    struct PortalSettings<T> {
                        a: T,
                    }
                ),
                "cannot be generic",
            ),
            (
                quote!(
                    struct Settings {
                        a: bool,
                    }
                ),
                "name the key",
            ),
            (
                quote!(
                    #[settings(permission = " ")]
                    struct PortalSettings {
                        a: bool,
                    }
                ),
                "must not be empty",
            ),
        ] {
            let out = expand_with_facade(input);
            assert!(out.contains(message), "expected `{message}` in {out}");
        }
    }
}
