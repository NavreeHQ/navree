//! `#[test]` (`#[navree::test]`, `#[navree_orm::test]`): an `async fn`
//! test run on a runtime of its own, given a database of its own when it
//! takes one.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, ExprLit, FnArg, ItemFn, Lit, Meta, ReturnType, Token};

use crate::paths;

/// Where the test's database comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Database {
    Sqlite,
    Memory,
    Postgres,
    MySql,
}

impl Database {
    fn parse(lit: &syn::LitStr) -> syn::Result<Self> {
        match lit.value().as_str() {
            "sqlite" => Ok(Database::Sqlite),
            "memory" => Ok(Database::Memory),
            "postgres" => Ok(Database::Postgres),
            "mysql" => Ok(Database::MySql),
            other => Err(syn::Error::new(
                lit.span(),
                format!(
                    "unknown database `{other}`: use \"sqlite\" (the default), \"memory\", \"postgres\" or \"mysql\""
                ),
            )),
        }
    }

    /// Whether the database may be missing (no server configured).
    fn on_server(self) -> bool {
        matches!(self, Database::Postgres | Database::MySql)
    }
}

/// `template = PATH, database = "..."`.
struct Options {
    template: Option<syn::Path>,
    database: Option<(Database, proc_macro2::Span)>,
}

impl Options {
    fn parse(args: TokenStream) -> syn::Result<Self> {
        let mut options = Options {
            template: None,
            database: None,
        };
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(args)?;
        for meta in metas {
            let Meta::NameValue(pair) = &meta else {
                return Err(syn::Error::new(
                    meta.span(),
                    "expected `template = PATH` or `database = \"...\"`",
                ));
            };
            if pair.path.is_ident("template") {
                let Expr::Path(path) = &pair.value else {
                    return Err(syn::Error::new(
                        pair.value.span(),
                        "`template` names a `static` Template: `template = DATABASE`",
                    ));
                };
                options.template = Some(path.path.clone());
            } else if pair.path.is_ident("database") {
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(lit), ..
                }) = &pair.value
                else {
                    return Err(syn::Error::new(
                        pair.value.span(),
                        "`database` is a string: \"sqlite\", \"memory\", \"postgres\" or \"mysql\"",
                    ));
                };
                options.database = Some((Database::parse(lit)?, lit.span()));
            } else {
                return Err(syn::Error::new(
                    pair.path.span(),
                    "unknown option: expected `template` or `database`",
                ));
            }
        }
        Ok(options)
    }
}

pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let options = Options::parse(args)?;
    let function: ItemFn = syn::parse2(item)?;
    let sig = &function.sig;
    if sig.asyncness.is_none() {
        return Err(syn::Error::new(
            sig.fn_token.span(),
            "`#[navree::test]` goes on an `async fn`",
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            sig.generics.span(),
            "a test function takes no generic parameters",
        ));
    }
    let krate = paths::orm(sig.ident.span())?;
    let database = options
        .database
        .map_or(Database::Sqlite, |(database, _)| database);
    if let (Some(template), Database::Memory) = (&options.template, database) {
        return Err(syn::Error::new(
            template.span(),
            "an in-memory database has no template: drop `template` or use \"sqlite\"",
        ));
    }
    let parameter = match sig.inputs.len() {
        0 => None,
        1 => match &sig.inputs[0] {
            FnArg::Typed(typed) => Some(typed),
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new(
                    receiver.span(),
                    "a test function takes no `self`",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                sig.inputs.span(),
                "a test function takes at most one parameter: its database (`db: TestDb`)",
            ));
        }
    };
    if parameter.is_none() && (options.template.is_some() || options.database.is_some()) {
        return Err(syn::Error::new(
            sig.ident.span(),
            "name the test's database as a parameter: `async fn name(db: TestDb)`",
        ));
    }

    let unit = matches!(sig.output, ReturnType::Default);
    let skip = if unit {
        quote!(())
    } else {
        quote!(::core::result::Result::Ok(()))
    };
    let setup = parameter.map(|typed| {
        let database_api = quote!(#krate::testing::database);
        let source = match (&options.template, database) {
            (Some(template), Database::Sqlite) => quote!(#template.sqlite()),
            (Some(template), Database::Postgres) => quote!(#template.postgres()),
            (Some(template), Database::MySql) => quote!(#template.mysql()),
            (None, Database::Sqlite) => quote!(#database_api::TestDb::sqlite()),
            (None, Database::Memory) => quote!(#database_api::TestDb::memory()),
            (None, Database::Postgres) => quote!(#database_api::TestDb::postgres()),
            (None, Database::MySql) => quote!(#database_api::TestDb::mysql()),
            (Some(_), Database::Memory) => unreachable!("refused above"),
        };
        let pat = &typed.pat;
        let ty = &typed.ty;
        let made = if database.on_server() {
            quote! {
                match #source.await {
                    ::core::result::Result::Ok(::core::option::Option::Some(db)) => db,
                    // No server configured: skipped (with a notice).
                    ::core::result::Result::Ok(::core::option::Option::None) => return #skip,
                    ::core::result::Result::Err(error) => {
                        ::core::panic!("cannot create the test database: {error}")
                    }
                }
            }
        } else {
            quote! {
                match #source.await {
                    ::core::result::Result::Ok(db) => db,
                    ::core::result::Result::Err(error) => {
                        ::core::panic!("cannot create the test database: {error}")
                    }
                }
            }
        };
        quote!(let #pat: #ty = #made;)
    });

    let attrs = &function.attrs;
    let vis = &function.vis;
    let ident = &sig.ident;
    let output = &sig.output;
    let block = &function.block;
    Ok(quote! {
        #(#attrs)*
        #[::core::prelude::v1::test]
        #vis fn #ident() #output {
            let runtime = #krate::__private::tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime for the test");
            runtime.block_on(async move {
                #setup
                #block
            })
        }
    })
}
