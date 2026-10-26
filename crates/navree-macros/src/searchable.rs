//! `#[derive(Searchable)]`: implements `navree_search::Searchable` for a
//! SeaORM model's `Entity` from the model's fields.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, LitInt, LitStr};

use crate::attr::{Errors, key_name};
use crate::paths::SEARCH;

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    try_expand(input).unwrap_or_else(syn::Error::into_compile_error)
}

/// The model attribute `#[searchable(name, index, soft_delete, workspace)]`.
#[derive(Default)]
struct ModelArgs {
    name: Option<LitStr>,
    index: Option<LitStr>,
    soft_delete: Option<LitStr>,
    workspace: Option<LitStr>,
}

/// One indexed field.
struct FieldArgs {
    ident: Ident,
    /// The field's name in the index.
    name: String,
    /// The column it comes from (the Rust field name).
    column: String,
    boost: Option<u8>,
    filter: bool,
    sort: bool,
    embed: bool,
}

fn try_expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let search = SEARCH.path(Span::call_site())?;
    let model = parse_model_args(&input)?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Searchable)] goes on a SeaORM model struct (`pub struct Model { .. }`)",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            data.fields.span(),
            "#[derive(Searchable)] needs a struct with named fields",
        ));
    };
    let mut errors = Errors::default();
    let mut fields: Vec<FieldArgs> = Vec::new();
    for field in &named.named {
        if let Some(args) = errors.keep(parse_field(field)).flatten() {
            if fields.iter().any(|f| f.name == args.name) {
                errors.push(syn::Error::new(
                    args.ident.span(),
                    format!("two fields are indexed as `{}`", args.name),
                ));
            }
            fields.push(args);
        }
    }
    errors.finish()?;

    let model_ident = &input.ident;
    let lit = |value: &str| LitStr::new(value, Span::call_site());
    let name = match &model.name {
        Some(name) => quote!(#name),
        None => quote!(table.clone()),
    };
    let index = match &model.index {
        Some(index) => quote!(#index),
        None => quote!(table.clone()),
    };
    let mut builder = Vec::new();
    for field in &fields {
        let field_name = lit(&field.name);
        if let Some(boost) = field.boost {
            builder.push(quote!(.text(#field_name, #boost)));
        }
        if field.filter {
            builder.push(quote!(.filter(#field_name)));
        }
        if field.sort {
            builder.push(quote!(.sort(#field_name)));
        }
        if field.embed {
            builder.push(quote!(.embed(#field_name)));
        }
        if field.column != field.name {
            let column = lit(&field.column);
            builder.push(quote!(.column(#field_name, #column)));
        }
    }
    if let Some(column) = &model.soft_delete {
        builder.push(quote!(.soft_delete(#column)));
    }
    let workspace = match &model.workspace {
        Some(column) => quote!(::core::option::Option::Some(#column)),
        None => quote!((&&#search::__private::WorkspaceProbe::<Entity>::new()).workspace_column()),
    };
    let document_fields = fields.iter().map(|field| {
        let field_name = lit(&field.name);
        let ident = &field.ident;
        quote!(.field(#field_name, &record.#ident))
    });

    Ok(quote! {
        #[automatically_derived]
        impl #search::Searchable for Entity {
            fn settings() -> #search::IndexSettings {
                #[allow(unused_imports)]
                use #search::__private::{ViaPlain as _, ViaScoped as _};
                let table = #search::__private::table_name::<Entity>();
                #search::IndexSettings::new(#name, #index)
                    #(#builder)*
                    .workspace_opt(#workspace)
            }

            fn document<'a>(
                record: &'a #model_ident,
                _ctx: &'a #search::SearchContext,
            ) -> #search::__private::BoxFuture<
                'a,
                ::core::result::Result<
                    ::core::option::Option<#search::Document>,
                    #search::__private::BoxError,
                >,
            > {
                ::std::boxed::Box::pin(async move {
                    ::core::result::Result::Ok(::core::option::Option::Some(
                        #search::Document::new(#search::__private::primary_key_string(record))
                            #(#document_fields)*,
                    ))
                })
            }
        }
    })
}

fn parse_model_args(input: &DeriveInput) -> syn::Result<ModelArgs> {
    let mut args = ModelArgs::default();
    for attr in &input.attrs {
        if !attr.path().is_ident("searchable") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let slot = match key_name(&meta.path).as_str() {
                "name" => &mut args.name,
                "index" => &mut args.index,
                "soft_delete" => &mut args.soft_delete,
                "workspace" => &mut args.workspace,
                other => {
                    return Err(meta.error(format!(
                        "unknown #[searchable] model argument `{other}`: expected `name`, \
                         `index`, `soft_delete` or `workspace`"
                    )));
                }
            };
            if slot.is_some() {
                return Err(meta.error("duplicate argument"));
            }
            let value: LitStr = meta.value()?.parse()?;
            if value.value().trim().is_empty() {
                return Err(syn::Error::new(value.span(), "must not be empty"));
            }
            *slot = Some(value);
            Ok(())
        })?;
    }
    Ok(args)
}

/// The field's `#[searchable(..)]` arguments; `None` for fields without
/// the attribute (they stay out of the index).
fn parse_field(field: &syn::Field) -> syn::Result<Option<FieldArgs>> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| syn::Error::new(field.span(), "expected a named field"))?;
    let mut found = false;
    let mut boost: Option<u8> = None;
    let (mut filter, mut sort, mut embed) = (false, false, false);
    let mut rename: Option<LitStr> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("searchable") {
            continue;
        }
        if found {
            return Err(syn::Error::new(
                attr.span(),
                "one #[searchable(..)] attribute per field: list every flag in it",
            ));
        }
        found = true;
        if matches!(attr.meta, syn::Meta::Path(_)) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            match key_name(&meta.path).as_str() {
                "boost" => {
                    let value: LitInt = meta.value()?.parse()?;
                    let n: u8 = value.base10_parse().ok().filter(|n| (1..=10).contains(n)).ok_or_else(
                        || syn::Error::new(value.span(), "the boost is a whole number from 1 to 10"),
                    )?;
                    boost = Some(n);
                }
                "filter" => filter = true,
                "sort" => sort = true,
                "embed" => embed = true,
                "rename" => {
                    let value: LitStr = meta.value()?.parse()?;
                    rename = Some(value);
                }
                other => {
                    return Err(meta.error(format!(
                        "unknown #[searchable] field argument `{other}`: expected `boost = N`, \
                         `filter`, `sort`, `embed` or `rename = \"..\"`"
                    )));
                }
            }
            Ok(())
        })?;
    }
    if !found {
        return Ok(None);
    }
    // A plain `#[searchable]` (or one with only `rename`) is a full-text
    // field of weight 1.
    if boost.is_none() && !filter && !sort && !embed {
        boost = Some(1);
    }
    let column = ident.to_string();
    let column = column.strip_prefix("r#").unwrap_or(&column).to_owned();
    let name = match &rename {
        Some(rename) => {
            let value = rename.value();
            if value.is_empty()
                || value.starts_with('_')
                || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(syn::Error::new(
                    rename.span(),
                    "a field name is letters, digits and `_`, not starting with `_`",
                ));
            }
            value
        }
        None => column.clone(),
    };
    Ok(Some(FieldArgs {
        ident,
        name,
        column,
        boost,
        filter,
        sort,
        embed,
    }))
}
