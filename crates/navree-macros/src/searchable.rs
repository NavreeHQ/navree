//! `#[derive(Searchable)]`: implements `navree_search::Searchable` for a
//! navree-orm model (`#[derive(Model)]`) from its fields.

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
    /// The column it comes from (`#[model(column = "..")]`, else the Rust
    /// field name).
    column: String,
    boost: Option<u8>,
    filter: bool,
    sort: bool,
    embed: bool,
}

fn try_expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let model = parse_model_args(&input)?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Searchable)] goes on a navree-orm model struct (`#[derive(Model)] pub struct Deal { .. }`)",
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
    let search = SEARCH.path(Span::call_site())?;

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
        None => {
            quote!((&&#search::__private::WorkspaceProbe::<#model_ident>::new()).workspace_column())
        }
    };
    // Filter and sort fields hold the column's value in the form access
    // rules compare (the database value: an enum's stored string, a
    // timestamp in UTC); text-only fields hold the serde form.
    let typed = fields.iter().any(|field| field.filter || field.sort);
    let columns = typed.then(|| quote!(let columns = #search::__private::columns(record);));
    let document_fields = fields.iter().map(|field| {
        let field_name = lit(&field.name);
        let ident = &field.ident;
        if field.filter || field.sort {
            let column = lit(&field.column);
            quote!(.field(#field_name, #search::__private::column(&columns, #column)))
        } else {
            quote!(.field(#field_name, &record.#ident))
        }
    });

    Ok(quote! {
        #[automatically_derived]
        impl #search::Searchable for #model_ident {
            fn settings() -> #search::IndexSettings {
                #[allow(unused_imports)]
                use #search::__private::{ViaPlain as _, ViaScoped as _};
                let table = #search::__private::table_name::<#model_ident>();
                #search::IndexSettings::new(#name, #index)
                    #(#builder)*
                    .workspace_opt(#workspace)
            }

            fn document<'a>(
                record: &'a #search::__private::Record<#model_ident>,
                _ctx: &'a #search::SearchContext,
            ) -> #search::__private::BoxFuture<
                'a,
                ::core::result::Result<
                    ::core::option::Option<#search::Document>,
                    #search::__private::BoxError,
                >,
            > {
                ::std::boxed::Box::pin(async move {
                    #columns
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
            return Err(syn::Error::new_spanned(
                attr,
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
                    let n: u8 = value
                        .base10_parse()
                        .ok()
                        .filter(|n| (1..=10).contains(n))
                        .ok_or_else(|| {
                            syn::Error::new(
                                value.span(),
                                "the boost is a whole number from 1 to 10",
                            )
                        })?;
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
    let rust_name = ident.to_string();
    let rust_name = rust_name
        .strip_prefix("r#")
        .unwrap_or(&rust_name)
        .to_owned();
    // The database column: `#[model(column = "..")]`, or the field. A field
    // that is not one column (`skip`, `columns(..)`) has no value to filter
    // or sort on.
    let model = model_attribute(field);
    if (filter || sort)
        && let Some(span) = model.not_a_column
    {
        return Err(syn::Error::new(
            span,
            "a filter or sort field is one column of the model: this field is not \
             (`#[model(skip)]` or `#[model(columns(..))]`); index it as text only, \
             or put the value in a column",
        ));
    }
    let column = model.column.unwrap_or_else(|| rust_name.clone());
    let name = match &rename {
        Some(rename) => {
            let value = rename.value();
            if value.is_empty()
                || value.starts_with('_')
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(syn::Error::new(
                    rename.span(),
                    "a field name is letters, digits and `_`, not starting with `_`",
                ));
            }
            value
        }
        None => rust_name,
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

/// What a field's `#[model(..)]` attributes say about its column.
#[derive(Default)]
struct ModelAttribute {
    /// `column = ".."`.
    column: Option<String>,
    /// Where `skip` or `columns(..)` says the field is not one column.
    not_a_column: Option<Span>,
}

/// Reads `column = ".."`, `skip` and `columns(..)` from a field's
/// `#[model(..)]` attributes. The tokens are scanned rather than parsed, so
/// the derive's other arguments of any shape pass through.
fn model_attribute(field: &syn::Field) -> ModelAttribute {
    use proc_macro2::{Delimiter, TokenTree};
    let mut out = ModelAttribute::default();
    for list in field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("model"))
        .filter_map(|attr| attr.meta.require_list().ok())
    {
        let tokens: Vec<TokenTree> = list.tokens.clone().into_iter().collect();
        for (at, token) in tokens.iter().enumerate() {
            let TokenTree::Ident(key) = token else {
                continue;
            };
            // Only top-level keys: the token before is a comma or nothing.
            let top =
                at == 0 || matches!(&tokens[at - 1], TokenTree::Punct(p) if p.as_char() == ',');
            if !top {
                continue;
            }
            let next = tokens.get(at + 1);
            match key.to_string().as_str() {
                "column" => {
                    if let (Some(TokenTree::Punct(eq)), Some(TokenTree::Literal(value))) =
                        (next, tokens.get(at + 2))
                        && eq.as_char() == '='
                        && let Ok(lit) = syn::parse_str::<LitStr>(&value.to_string())
                    {
                        out.column = Some(lit.value());
                    }
                }
                "skip" => out.not_a_column = Some(key.span()),
                "columns" if matches!(next, Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis) =>
                {
                    out.not_a_column = Some(key.span());
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(tokens: TokenStream) -> syn::Result<Option<FieldArgs>> {
        let input: DeriveInput = syn::parse2(quote!(struct Model { #tokens }))?;
        let Data::Struct(data) = input.data else {
            unreachable!()
        };
        parse_field(data.fields.iter().next().expect("one field"))
    }

    fn error(tokens: TokenStream) -> String {
        match field(tokens) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("expected an error"),
        }
    }

    #[test]
    fn fields_without_the_attribute_stay_out() {
        assert!(
            field(quote!(pub updated_at: DateTimeUtc))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_plain_attribute_is_a_text_field_of_weight_one() {
        let args = field(quote!(#[searchable] pub phone: Option<String>))
            .unwrap()
            .unwrap();
        assert_eq!((args.name.as_str(), args.boost), ("phone", Some(1)));
        let args = field(quote!(#[searchable(rename = "email_address")] pub email: String))
            .unwrap()
            .unwrap();
        assert_eq!(
            (args.name.as_str(), args.column.as_str(), args.boost),
            ("email_address", "email", Some(1))
        );
    }

    #[test]
    fn the_column_comes_from_model_column() {
        let args = field(quote!(
            #[model(cast = "string_enum", column = "mail", hidden)]
            #[searchable(filter)]
            pub email: Option<String>
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            (args.name.as_str(), args.column.as_str()),
            ("email", "mail")
        );
        let args = field(quote!(
            #[model(default)]
            #[searchable(filter)]
            pub r#type: String
        ))
        .unwrap()
        .unwrap();
        assert_eq!((args.name.as_str(), args.column.as_str()), ("type", "type"));
        // A key named `column` inside another argument is not the field's.
        let args = field(quote!(
            #[model(cast(Wrapped, column = "inner"))]
            #[searchable]
            pub notes: String
        ))
        .unwrap()
        .unwrap();
        assert_eq!(args.column, "notes");
    }

    #[test]
    fn filters_and_sorts_need_one_column() {
        let message = error(quote!(
            #[model(skip)]
            #[searchable(filter)]
            pub label: String
        ));
        assert!(message.contains("one column of the model"), "{message}");
        let message = error(quote!(
            #[model(columns(amount: i64, currency: String))]
            #[searchable(sort)]
            pub price: Money
        ));
        assert!(message.contains("one column of the model"), "{message}");
        // As text only, a computed field is fine.
        let args = field(quote!(
            #[model(skip)]
            #[searchable(boost = 2)]
            pub label: String
        ))
        .unwrap()
        .unwrap();
        assert_eq!(args.boost, Some(2));
    }

    #[test]
    fn flags_combine() {
        let args = field(quote!(#[searchable(boost = 2, embed)] pub title: String))
            .unwrap()
            .unwrap();
        assert_eq!(args.boost, Some(2));
        assert!(args.embed && !args.filter);
        let args = field(quote!(#[searchable(filter, sort)] pub score: i64))
            .unwrap()
            .unwrap();
        assert!(args.filter && args.sort && args.boost.is_none());
    }

    #[test]
    fn mistakes_are_reported() {
        assert!(error(quote!(#[searchable(boost = 11)] pub a: String)).contains("1 to 10"));
        assert!(error(quote!(#[searchable(boost = 0)] pub a: String)).contains("1 to 10"));
        assert!(
            error(quote!(#[searchable(fuzzy)] pub a: String))
                .contains("unknown #[searchable] field argument `fuzzy`")
        );
        assert!(
            error(quote!(#[searchable(rename = "_x")] pub a: String)).contains("letters, digits")
        );
        assert!(
            error(quote!(#[searchable] #[searchable(filter)] pub a: String))
                .contains("one #[searchable(..)]")
        );
    }

    #[test]
    fn model_arguments_are_checked() {
        let input: DeriveInput = syn::parse2(quote! {
            #[searchable(name = "Contact", index = "people", soft_delete = "deleted_at", workspace = "team_id")]
            struct Model { id: u32 }
        })
        .unwrap();
        let args = parse_model_args(&input).unwrap();
        assert_eq!(args.name.unwrap().value(), "Contact");
        assert_eq!(args.index.unwrap().value(), "people");
        assert_eq!(args.soft_delete.unwrap().value(), "deleted_at");
        assert_eq!(args.workspace.unwrap().value(), "team_id");

        let input: DeriveInput = syn::parse2(quote!(
            #[searchable(table = "x")]
            struct Model {
                id: u32,
            }
        ))
        .unwrap();
        let error = parse_model_args(&input).err().unwrap().to_string();
        assert!(
            error.contains("unknown #[searchable] model argument `table`"),
            "{error}"
        );
        let input: DeriveInput = syn::parse2(quote!(
            #[searchable(name = "")]
            struct Model {
                id: u32,
            }
        ))
        .unwrap();
        assert!(parse_model_args(&input).is_err());
    }

    #[test]
    fn enums_and_tuple_structs_are_refused() {
        let error = try_expand(quote!(
            enum Model {
                A,
            }
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("navree-orm model struct"), "{error}");
        let error = try_expand(quote!(
            struct Model(u32);
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("named fields"), "{error}");
        let error = try_expand(quote!(
            struct Model {
                #[searchable]
                a: String,
                #[searchable(rename = "a")]
                b: String,
            }
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("two fields are indexed as `a`"), "{error}");
    }
}
