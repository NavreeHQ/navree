//! `#[module]`: implements `Module` from a declarative list of imports,
//! providers, controllers and exports.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, Ident, Item, ItemStruct, Token, Type, token};

use crate::attr::key_name;
use crate::config_section::config_path;
use crate::paths::{HTTP, KERNEL};

/// Expands `#[module(args)] item`. On error the item is kept so the error
/// does not cascade into "cannot find type" errors.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    match try_expand(args, item.clone()) {
        Ok(expanded) => expanded,
        Err(error) => {
            let mut out = error.into_compile_error();
            out.extend(item);
            out
        }
    }
}

fn try_expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = ModuleArgs::parse(args)?;
    let item = parse_struct(item)?;
    let kernel = KERNEL.path(Span::call_site())?;
    generate(&args, &item, &kernel, |span| HTTP.path(span), config_path)
}

/// What `#[module(...)]` declares.
#[derive(Default)]
struct ModuleArgs {
    imports: Vec<Expr>,
    providers: Vec<Type>,
    /// The span of the `controllers` key (for "missing navree-http" errors) and the types.
    controllers: Option<(Span, Vec<Type>)>,
    exports: Vec<Type>,
    /// The span of the `config` key (for "missing navree-config" errors) and
    /// the configuration sections (`ConfigSection` types).
    config: Option<(Span, Vec<Type>)>,
    /// The span of the `global` flag, when present.
    global: Option<Span>,
    /// `register = f`: called last with `(&self, &mut ModuleBuilder)`.
    register: Option<Expr>,
}

/// The keys `#[module]` accepts, for error messages.
const KEYS: &str =
    "`imports`, `providers`, `controllers`, `exports`, `config`, `global` or `register`";

impl ModuleArgs {
    fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut parsed = ModuleArgs::default();
        let mut seen: Vec<String> = Vec::new();
        let parser = syn::meta::parser(|meta| {
            let key = key_name(&meta.path);
            if seen.contains(&key) {
                return Err(meta.error(format!("duplicate `{key}` key")));
            }
            parsed.parse_key(&key, &meta)?;
            seen.push(key);
            Ok(())
        });
        syn::parse::Parser::parse2(parser, args)?;
        Ok(parsed)
    }

    fn parse_key(&mut self, key: &str, meta: &ParseNestedMeta) -> syn::Result<()> {
        match key {
            "imports" => self.imports = list(meta, key)?,
            "providers" => self.providers = list(meta, key)?,
            "controllers" => self.controllers = Some((meta.path.span(), list(meta, key)?)),
            "exports" => self.exports = list(meta, key)?,
            "config" => self.config = Some((meta.path.span(), list(meta, key)?)),
            "global" => {
                if meta.input.peek(Token![=]) || meta.input.peek(token::Paren) {
                    return Err(meta.error("`global` is a flag and takes no value: write `global`"));
                }
                self.global = Some(meta.path.span());
            }
            "register" => self.register = Some(meta.value()?.parse()?),
            _ => {
                let hint = suggestion(key)
                    .map(|known| format!("did you mean `{known}`? ("))
                    .unwrap_or_default();
                let close = if hint.is_empty() { "" } else { ")" };
                return Err(meta.error(format!(
                    "unknown #[module] key `{key}`: {hint}expected {KEYS}{close}"
                )));
            }
        }
        Ok(())
    }
}

/// The known key `key` is probably a typo of: a singular/plural slip or a
/// case difference (`import` → `imports`, `Global` → `global`).
fn suggestion(key: &str) -> Option<&'static str> {
    const KNOWN: [&str; 7] = [
        "imports",
        "providers",
        "controllers",
        "exports",
        "config",
        "global",
        "register",
    ];
    let key = key.to_ascii_lowercase();
    KNOWN
        .into_iter()
        .find(|known| *known == key || *known == format!("{key}s") || key == format!("{known}s"))
}

/// Parses `= [a, b, ...]` after `key`.
fn list<T: Parse>(meta: &ParseNestedMeta, key: &str) -> syn::Result<Vec<T>> {
    if !meta.input.peek(Token![=]) {
        return Err(meta.error(format!("expected a list: `{key} = [...]`")));
    }
    let input = meta.value()?;
    if !input.peek(token::Bracket) {
        return Err(input.error(format!(
            "expected a list in square brackets: `{key} = [...]`"
        )));
    }
    let content;
    syn::bracketed!(content in input);
    let items = content.call(Punctuated::<T, Token![,]>::parse_terminated)?;
    Ok(items.into_iter().collect())
}

/// Parses the annotated item, which must be a struct.
fn parse_struct(item: TokenStream) -> syn::Result<ItemStruct> {
    let span = match syn::parse2::<Item>(item)? {
        Item::Struct(item) => return Ok(item),
        Item::Enum(item) => item.enum_token.span,
        Item::Union(item) => item.union_token.span,
        other => other.span(),
    };
    Err(syn::Error::new(
        span,
        "#[module] can only be used on a struct",
    ))
}

/// Generates the item plus its `impl Module`. `http` resolves the navree-http
/// path; it is only called when the module declares controllers. `config`
/// resolves the navree-config path; it is only called when the module
/// declares configuration sections.
fn generate(
    args: &ModuleArgs,
    item: &ItemStruct,
    kernel: &TokenStream,
    http: impl FnOnce(Span) -> syn::Result<TokenStream>,
    config: impl FnOnce(Span) -> syn::Result<TokenStream>,
) -> syn::Result<TokenStream> {
    // Mixed-site hygiene: expressions written by the user cannot see (or
    // shadow) the builder variable.
    let builder = Ident::new("builder", Span::mixed_site());

    let imports = args
        .imports
        .iter()
        .map(|module| quote_spanned!(module.span()=> #builder.import(#module);));
    let sections = match &args.config {
        Some((span, types)) if !types.is_empty() => {
            let config = config(*span)?;
            types
                .iter()
                .map(|ty| {
                    quote_spanned!(ty.span()=>
                        #config::ModuleBuilderConfigExt::config::<#ty>(#builder);
                    )
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let providers = args
        .providers
        .iter()
        .map(|ty| quote_spanned!(ty.span()=> #builder.provider::<#ty>();));
    let controllers = match &args.controllers {
        Some((span, types)) if !types.is_empty() => {
            let http = http(*span)?;
            types
                .iter()
                .map(|ty| {
                    quote_spanned!(ty.span()=>
                        #http::ModuleBuilderHttpExt::controller::<#ty>(#builder);
                    )
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let exports = args
        .exports
        .iter()
        .map(|ty| quote_spanned!(ty.span()=> #builder.exports::<#ty>();));
    let global = args
        .global
        .map(|span| quote_spanned!(span=> #builder.global();));
    // Called through a generic helper so a closure gets its parameter types
    // from the `FnOnce(&Self, &mut ModuleBuilder)` bound.
    let register = args.register.as_ref().map(|f| {
        quote_spanned! {f.span()=>
            {
                fn __navree_register<M: ?::core::marker::Sized>(
                    module: &M,
                    builder: &mut #kernel::ModuleBuilder,
                    f: impl ::core::ops::FnOnce(&M, &mut #kernel::ModuleBuilder),
                ) {
                    f(module, builder);
                }
                __navree_register(self, #builder, #f);
            }
        }
    });

    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let item = item.to_token_stream();
    Ok(quote! {
        #item

        impl #impl_generics #kernel::Module for #name #ty_generics #where_clause {
            fn register(&self, #builder: &mut #kernel::ModuleBuilder) {
                #(#imports)*
                #(#sections)*
                #(#providers)*
                #(#controllers)*
                #(#exports)*
                #global
                #register
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_with_http(
        args: TokenStream,
        http: impl FnOnce(Span) -> syn::Result<TokenStream>,
    ) -> syn::Result<String> {
        let args = ModuleArgs::parse(args)?;
        let item: ItemStruct = syn::parse_quote!(
            struct UsersModule;
        );
        generate(&args, &item, &quote!(::navree_kernel), http, |_| {
            Ok(quote!(::navree_config))
        })
        .map(|tokens| tokens.to_string())
    }

    #[test]
    fn config_sections_register_through_the_config_extension() {
        let out = expand_with_http(quote!(config = [MailOptions, admin::AuditOptions]), |_| {
            panic!("navree-http must not be looked up")
        })
        .expect("expands");
        for section in ["MailOptions", "admin :: AuditOptions"] {
            let call = format!(
                ":: navree_config :: ModuleBuilderConfigExt :: config :: < {section} > (builder) ;"
            );
            assert!(out.contains(&call), "missing `{call}` in {out}");
        }
    }

    #[test]
    fn modules_without_config_do_not_need_the_config_crate() {
        let args = ModuleArgs::parse(quote!(providers = [Repo], config = [])).expect("parses");
        let item: ItemStruct = syn::parse_quote!(
            struct UsersModule;
        );
        generate(
            &args,
            &item,
            &quote!(::navree_kernel),
            |_| panic!("navree-http must not be looked up"),
            |_| panic!("navree-config must not be looked up"),
        )
        .expect("expands");
    }

    #[test]
    fn controllers_register_through_the_http_extension() {
        let out = expand_with_http(
            quote!(controllers = [UserController, admin::AuditController]),
            |_| Ok(quote!(::navree_http)),
        )
        .expect("expands");
        for controller in ["UserController", "admin :: AuditController"] {
            let call = format!(
                ":: navree_http :: ModuleBuilderHttpExt :: controller :: < {controller} > (builder) ;"
            );
            assert!(out.contains(&call), "missing `{call}` in {out}");
        }
    }

    #[test]
    fn controllers_need_the_http_crate() {
        let error = expand_with_http(quote!(controllers = [UserController]), |_| {
            Err(syn::Error::new(Span::call_site(), "needs navree-http"))
        })
        .expect_err("navree-http is missing");
        assert_eq!(error.to_string(), "needs navree-http");
    }

    #[test]
    fn modules_without_controllers_do_not_need_the_http_crate() {
        for args in [quote!(providers = [Repo]), quote!(controllers = [])] {
            expand_with_http(args, |_| panic!("navree-http must not be looked up"))
                .expect("expands");
        }
    }
}
