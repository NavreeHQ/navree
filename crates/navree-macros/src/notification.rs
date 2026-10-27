//! `#[notification]`: sets the category, digest and urgency of a
//! `navree_notify::Notification` implementation.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{ImplItem, ItemImpl, LitStr, Token};

use crate::attr::{Errors, key_name};
use crate::paths::NOTIFY;

/// The longest category name (navree-notify's `MAX_CATEGORY_LEN`).
const MAX_CATEGORY_LEN: usize = 100;

/// navree-notify's `Digest` variants by their names.
const DIGESTS: [(&str, &str); 4] = [
    ("off", "Off"),
    ("hourly", "Hourly"),
    ("daily", "Daily"),
    ("weekly", "Weekly"),
];

/// Expands `#[notification(args)] impl Notification for X { .. }`. On a
/// mistake, the item is emitted unchanged next to the error.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    let parsed = match parse(args, item.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(item);
            return out;
        }
    };
    match NOTIFY.path(Span::call_site()) {
        Ok(notify) => generate(parsed, &notify),
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(parsed.item.into_token_stream());
            out
        }
    }
}

/// A parsed `#[notification]`.
pub(crate) struct ParsedNotification {
    item: ItemImpl,
    notification_type: Option<LitStr>,
    category: Option<LitStr>,
    /// The `Digest` variant (`Daily`).
    digest: Option<&'static str>,
    urgent: bool,
}

/// `type = "..", category = "..", digest = "daily", urgent`.
#[derive(Default)]
struct Args {
    notification_type: Option<LitStr>,
    category: Option<LitStr>,
    digest: Option<LitStr>,
    urgent: Option<Span>,
}

impl Args {
    fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut parsed = Args::default();
        let mut seen: Vec<String> = Vec::new();
        let parser = syn::meta::parser(|meta| {
            let key = if meta.path.is_ident("type") {
                "type".to_owned()
            } else {
                key_name(&meta.path)
            };
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            match key.as_str() {
                "type" => parsed.notification_type = Some(meta.value()?.parse()?),
                "category" => parsed.category = Some(meta.value()?.parse()?),
                "digest" => parsed.digest = Some(meta.value()?.parse()?),
                "urgent" => {
                    if meta.input.peek(Token![=]) {
                        return Err(meta.error("`urgent` takes no value: write `urgent`"));
                    }
                    parsed.urgent = Some(meta.path.segments[0].ident.span());
                }
                _ => {
                    return Err(meta.error(format!(
                        "unknown #[notification] key `{key}`: expected `type = \"..\"`, \
                         `category = \"..\"`, `digest = \"daily\"` or `urgent`"
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

/// Parses the arguments and the impl block.
pub(crate) fn parse(args: TokenStream, item: TokenStream) -> syn::Result<ParsedNotification> {
    let item: ItemImpl = syn::parse2(item).map_err(|error| {
        syn::Error::new(
            error.span(),
            "#[notification] goes on an `impl Notification for ..` block",
        )
    })?;
    let mut errors = Errors::default();
    let names_notification = item.trait_.as_ref().is_some_and(|(path, _)| {
        path.segments
            .last()
            .is_some_and(|segment| segment.ident == "Notification")
    });
    if !names_notification {
        errors.push(syn::Error::new_spanned(
            &item.self_ty,
            "#[notification] goes on an `impl Notification for ..` block",
        ));
    }
    let args = errors.keep(Args::parse(args)).unwrap_or_default();
    let defined = |name: &str| {
        item.items.iter().find_map(|member| match member {
            ImplItem::Const(constant) if constant.ident == name => Some(constant.ident.span()),
            _ => None,
        })
    };
    for (key, constant, given) in [
        ("type", "TYPE", args.notification_type.is_some()),
        ("category", "CATEGORY", args.category.is_some()),
        ("digest", "DIGEST", args.digest.is_some()),
        ("urgent", "URGENT", args.urgent.is_some()),
    ] {
        if given && let Some(span) = defined(constant) {
            errors.push(syn::Error::new(
                span,
                format!("`{constant}` is set twice: by `{key}` in #[notification] and here"),
            ));
        }
    }
    if let Some(category) = &args.category {
        errors.keep(
            check_category(&category.value()).map_err(|m| syn::Error::new(category.span(), m)),
        );
    }
    let digest = match &args.digest {
        Some(digest) => {
            let found = DIGESTS
                .iter()
                .find(|(name, _)| *name == digest.value())
                .map(|(_, variant)| *variant);
            if found.is_none() {
                errors.push(syn::Error::new(
                    digest.span(),
                    format!(
                        "unknown digest `{}`: use `off`, `hourly`, `daily` or `weekly`",
                        digest.value()
                    ),
                ));
            }
            found
        }
        None => None,
    };
    if let (Some(span), Some(variant)) = (args.urgent, digest)
        && variant != "Off"
    {
        errors.push(syn::Error::new(
            span,
            "an urgent notification is never batched into a digest: drop `digest` or `urgent`",
        ));
    }
    errors.finish()?;
    Ok(ParsedNotification {
        item,
        notification_type: args.notification_type,
        category: args.category,
        digest,
        urgent: args.urgent.is_some(),
    })
}

/// The impl block with the constants the arguments set, naming
/// navree-notify as `notify`.
pub(crate) fn generate(parsed: ParsedNotification, notify: &TokenStream) -> TokenStream {
    let mut item = parsed.item;
    let mut constants: Vec<ImplItem> = Vec::new();
    if let Some(notification_type) = &parsed.notification_type {
        constants.push(syn::parse_quote! {
            const TYPE: &'static str = #notification_type;
        });
    }
    if let Some(category) = &parsed.category {
        constants.push(syn::parse_quote! {
            const CATEGORY: &'static str = #category;
        });
    }
    if let Some(variant) = parsed.digest {
        let variant = format_ident!("{variant}");
        constants.push(syn::parse_quote! {
            const DIGEST: #notify::Digest = #notify::Digest::#variant;
        });
    }
    if parsed.urgent {
        constants.push(syn::parse_quote! {
            const URGENT: bool = true;
        });
    }
    constants.append(&mut item.items);
    item.items = constants;
    quote!(#item)
}

/// navree-notify's category rule: 1 to 100 ASCII letters, digits, `_`,
/// `-`, `.` or `:`.
fn check_category(category: &str) -> Result<(), String> {
    let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':');
    if category.is_empty() || category.len() > MAX_CATEGORY_LEN || !category.chars().all(valid) {
        return Err(format!(
            "invalid category `{category}`: use 1 to {MAX_CATEGORY_LEN} letters, digits, `_`, \
             `-`, `.` or `:` (`deals`)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_with_facade(args: TokenStream, item: TokenStream) -> String {
        match parse(args, item) {
            Ok(parsed) => generate(parsed, &quote!(::navree::notify)).to_string(),
            Err(error) => format!("error: {error}"),
        }
    }

    fn deal_won() -> TokenStream {
        quote! {
            impl Notification for DealWon {
                const TYPE: &'static str = "deal_won";

                fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
                    vec![Channel::Mail]
                }
            }
        }
    }

    #[test]
    fn the_arguments_become_constants_of_the_impl() {
        let out = expand_with_facade(quote!(category = "deals", digest = "daily"), deal_won());
        assert_eq!(
            out,
            quote! {
                impl Notification for DealWon {
                    const CATEGORY: &'static str = "deals";
                    const DIGEST: ::navree::notify::Digest = ::navree::notify::Digest::Daily;
                    const TYPE: &'static str = "deal_won";

                    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> {
                        vec![Channel::Mail]
                    }
                }
            }
            .to_string()
        );
    }

    #[test]
    fn the_type_and_urgency_can_be_given_too() {
        let out = expand_with_facade(
            quote!(type = "sign_in_code", category = "security", urgent),
            quote! {
                impl navree::notify::Notification for SignInCode {
                    fn via(&self, _n: &dyn Notifiable) -> Vec<Channel> { vec![] }
                }
            },
        );
        assert!(out.contains("const TYPE : & 'static str = \"sign_in_code\""), "{out}");
        assert!(out.contains("const CATEGORY : & 'static str = \"security\""), "{out}");
        assert!(out.contains("const URGENT : bool = true"), "{out}");
        assert!(!out.contains("DIGEST"), "{out}");
        // No arguments: the impl as it is.
        assert_eq!(expand_with_facade(quote!(), deal_won()), deal_won().to_string());
        // An urgent notification may say `digest = "off"`.
        let out = expand_with_facade(quote!(digest = "off", urgent), deal_won());
        assert!(out.contains("Digest :: Off"), "{out}");
    }

    #[test]
    fn misuse_is_reported() {
        for (args, item, message) in [
            (
                quote!(category = "deals"),
                quote!(
                    struct DealWon;
                ),
                "goes on an `impl Notification for ..` block",
            ),
            (
                quote!(category = "deals"),
                quote!(impl Mailable for DealWon {}),
                "goes on an `impl Notification for ..` block",
            ),
            (
                quote!(label = "x"),
                deal_won(),
                "unknown #[notification] key `label`",
            ),
            (
                quote!(digest = "monthly"),
                deal_won(),
                "unknown digest `monthly`",
            ),
            (
                quote!(category = "big deals"),
                deal_won(),
                "invalid category `big deals`",
            ),
            (
                quote!(type = "x"),
                deal_won(),
                "`TYPE` is set twice",
            ),
            (
                quote!(digest = "daily", urgent),
                deal_won(),
                "never batched",
            ),
            (
                quote!(urgent = true),
                deal_won(),
                "`urgent` takes no value",
            ),
            (
                quote!(category = "a", category = "b"),
                deal_won(),
                "duplicate `category` key",
            ),
        ] {
            let out = expand_with_facade(args, item);
            assert!(out.starts_with("error:") && out.contains(message), "{message}: {out}");
        }
    }
}
