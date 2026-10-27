//! `#[derive(Exportable)]` and `#[derive(Importable)]`: the row types of
//! navree-sheets' exports and imports.

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Ident, Lit, LitStr, Path, Type};

use crate::attr::{Errors, key_name};
use crate::paths::SHEETS;

/// `first_name` -> `First name` (the same as `navree_sheets::sentence_case`).
fn sentence_case(field: &str) -> String {
    let words = field.trim_matches('_').replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The Rust name of a field, without `r#`.
fn field_name(ident: &Ident) -> String {
    let name = ident.to_string();
    name.strip_prefix("r#").unwrap_or(&name).to_owned()
}

/// The named fields of a struct, or an error naming the derive.
fn named_fields<'a>(
    input: &'a DeriveInput,
    derive: &str,
) -> syn::Result<&'a syn::punctuated::Punctuated<syn::Field, syn::Token![,]>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("#[derive({derive})] goes on a struct with named fields"),
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            data.fields.span(),
            format!("#[derive({derive})] needs a struct with named fields"),
        ));
    };
    Ok(&named.named)
}

fn non_empty(value: LitStr) -> syn::Result<LitStr> {
    if value.value().trim().is_empty() {
        Err(syn::Error::new(value.span(), "must not be empty"))
    } else {
        Ok(value)
    }
}

// ---------------------------------------------------------------------------
// #[derive(Exportable)]

struct ExportField {
    ident: Ident,
    key: String,
    header: LitStr,
    format: Option<LitStr>,
    width: Option<f64>,
    with: Option<Path>,
}

pub(crate) fn expand_exportable(input: TokenStream) -> TokenStream {
    try_expand_exportable(input, || SHEETS.path(Span::call_site()))
        .unwrap_or_else(syn::Error::into_compile_error)
}

/// The expansion, with `sheets` giving the path to navree-sheets (resolved
/// after the input parsed, so input errors come first).
fn try_expand_exportable(
    input: TokenStream,
    sheets: impl FnOnce() -> syn::Result<TokenStream>,
) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let sheet = parse_export_struct(&input)?;
    let fields = named_fields(&input, "Exportable")?;
    let mut errors = Errors::default();
    let mut columns = Vec::new();
    for field in fields {
        if let Some(column) = errors.keep(parse_export_field(field)).flatten() {
            columns.push(column);
        }
    }
    errors.finish()?;
    let sheets = sheets()?;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let column_defs = columns.iter().map(|column| {
        let key = &column.key;
        let header = &column.header;
        let format = column.format.as_ref().map(|f| quote!(.format(#f)));
        let width = column.width.map(|w| quote!(.width(#w)));
        quote!(#sheets::Column::new(#key, #header) #format #width)
    });
    let cells = columns.iter().map(|column| {
        let field = &column.ident;
        match &column.with {
            Some(path) => quote!(#path(&self.#field)),
            None => quote!(#sheets::IntoCell::to_cell(&self.#field)),
        }
    });
    let sheet_name = sheet.map(|sheet| {
        quote! {
            fn sheet_name() -> &'static str {
                #sheet
            }
        }
    });
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #sheets::Exportable for #ident #ty_generics #where_clause {
            fn columns() -> ::std::vec::Vec<#sheets::Column> {
                ::std::vec![#(#column_defs),*]
            }

            fn cells(&self) -> ::std::vec::Vec<#sheets::Cell> {
                ::std::vec![#(#cells),*]
            }

            #sheet_name
        }
    })
}

fn parse_export_struct(input: &DeriveInput) -> syn::Result<Option<LitStr>> {
    let mut sheet = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("export") {
            continue;
        }
        attr.parse_nested_meta(|meta| match key_name(&meta.path).as_str() {
            "sheet" => {
                if sheet.is_some() {
                    return Err(meta.error("duplicate argument"));
                }
                sheet = Some(non_empty(meta.value()?.parse()?)?);
                Ok(())
            }
            other => Err(meta.error(format!(
                "unknown #[export] struct argument `{other}`: expected `sheet = \"..\"`"
            ))),
        })?;
    }
    Ok(sheet)
}

/// The field's column; `None` for `#[export(skip)]`.
fn parse_export_field(field: &syn::Field) -> syn::Result<Option<ExportField>> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| syn::Error::new(field.span(), "expected a named field"))?;
    let key = field_name(&ident);
    let mut header: Option<LitStr> = None;
    let mut format: Option<LitStr> = None;
    let mut width: Option<f64> = None;
    let mut with: Option<Path> = None;
    let mut skip = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("export") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            match key_name(&meta.path).as_str() {
                "header" => header = Some(non_empty(meta.value()?.parse()?)?),
                "format" => format = Some(non_empty(meta.value()?.parse()?)?),
                "width" => {
                    let value: Lit = meta.value()?.parse()?;
                    let parsed = match &value {
                        Lit::Int(int) => int.base10_parse::<f64>().ok(),
                        Lit::Float(float) => float.base10_parse::<f64>().ok(),
                        _ => None,
                    };
                    width = Some(parsed.filter(|w| *w > 0.0 && *w <= 255.0).ok_or_else(|| {
                        syn::Error::new(value.span(), "the width is a number from 1 to 255")
                    })?);
                }
                "with" => {
                    let value: LitStr = meta.value()?.parse()?;
                    with = Some(value.parse()?);
                }
                "skip" => skip = true,
                other => {
                    return Err(meta.error(format!(
                        "unknown #[export] field argument `{other}`: expected `header = \"..\"`, \
                         `format = \"..\"`, `width = N`, `with = \"path\"` or `skip`"
                    )));
                }
            }
            Ok(())
        })?;
    }
    if skip {
        if header.is_some() || format.is_some() || width.is_some() || with.is_some() {
            return Err(syn::Error::new(
                ident.span(),
                "a skipped field takes no other #[export] argument",
            ));
        }
        return Ok(None);
    }
    let header = header.unwrap_or_else(|| LitStr::new(&sentence_case(&key), ident.span()));
    Ok(Some(ExportField {
        ident,
        key,
        header,
        format,
        width,
        with,
    }))
}

// ---------------------------------------------------------------------------
// #[derive(Importable)]

struct ImportField {
    key: String,
    header: LitStr,
    aliases: Vec<LitStr>,
    required: bool,
}

pub(crate) fn expand_importable(input: TokenStream) -> TokenStream {
    try_expand_importable(input, || SHEETS.path(Span::call_site()))
        .unwrap_or_else(syn::Error::into_compile_error)
}

/// The expansion, with `sheets` giving the path to navree-sheets.
fn try_expand_importable(
    input: TokenStream,
    sheets: impl FnOnce() -> syn::Result<TokenStream>,
) -> syn::Result<TokenStream> {
    let input: DeriveInput = syn::parse2(input)?;
    let serde = SerdeStruct::of(&input)?;
    let fields = named_fields(&input, "Importable")?;
    let mut errors = Errors::default();
    let mut columns: Vec<ImportField> = Vec::new();
    for field in fields {
        if let Some(column) = errors.keep(parse_import_field(field, &serde)).flatten() {
            if columns.iter().any(|c| c.key == column.key) {
                errors.push(syn::Error::new(
                    field.span(),
                    format!("two fields read the column `{}`", column.key),
                ));
            }
            columns.push(column);
        }
    }
    if !errors.has_errors() && columns.is_empty() {
        errors.push(syn::Error::new(
            input.ident.span(),
            "#[derive(Importable)] needs at least one field that is not skipped",
        ));
    }
    errors.finish()?;
    let sheets = sheets()?;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let defs = columns.iter().map(|column| {
        let key = &column.key;
        let header = &column.header;
        let aliases = &column.aliases;
        let required = column.required;
        quote!(#sheets::ImportColumn::new(#key, #header) #(.alias(#aliases))* .required(#required))
    });
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #sheets::ImportRow for #ident #ty_generics #where_clause {
            fn columns() -> ::std::vec::Vec<#sheets::ImportColumn> {
                ::std::vec![#(#defs),*]
            }
        }
    })
}

/// The struct-level serde arguments the derive reads.
struct SerdeStruct {
    /// `#[serde(default)]` on the struct: every field may be missing.
    default: bool,
}

impl SerdeStruct {
    fn of(input: &DeriveInput) -> syn::Result<Self> {
        let mut default = false;
        for attr in input.attrs.iter().filter(|a| a.path().is_ident("serde")) {
            let Ok(list) = attr.meta.require_list() else {
                continue;
            };
            for key in top_level_keys(list.tokens.clone()) {
                match key.as_str() {
                    "default" => default = true,
                    "rename_all" | "rename_all_fields" => {
                        return Err(syn::Error::new_spanned(
                            attr,
                            "#[derive(Importable)] reads each field's own name: rename fields one \
                             by one with #[serde(rename = \"..\")] instead of rename_all",
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(SerdeStruct { default })
    }
}

/// The keys of a `key, key = value, key(..)` list, at its top level.
fn top_level_keys(tokens: TokenStream) -> Vec<String> {
    let mut keys = Vec::new();
    let mut expect_key = true;
    for token in tokens {
        match token {
            TokenTree::Ident(ident) if expect_key => {
                keys.push(ident.to_string());
                expect_key = false;
            }
            TokenTree::Punct(punct) if punct.as_char() == ',' => expect_key = true,
            _ => {}
        }
    }
    keys
}

/// The `rename = ".."` (or `rename(deserialize = "..")`) of a field's serde
/// attributes, and whether it has `default` or skips deserializing.
struct SerdeField {
    rename: Option<String>,
    default: bool,
    skip: bool,
}

fn serde_field(field: &syn::Field) -> syn::Result<SerdeField> {
    let mut out = SerdeField {
        rename: None,
        default: false,
        skip: false,
    };
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            match key_name(&meta.path).as_str() {
                "rename" => {
                    if meta.input.peek(syn::Token![=]) {
                        let value: LitStr = meta.value()?.parse()?;
                        out.rename = Some(value.value());
                    } else {
                        meta.parse_nested_meta(|inner| {
                            let value: LitStr = inner.value()?.parse()?;
                            if inner.path.is_ident("deserialize") {
                                out.rename = Some(value.value());
                            }
                            Ok(())
                        })?;
                    }
                }
                "default" => {
                    out.default = true;
                    skip_value(&meta)?;
                }
                "skip" | "skip_deserializing" => out.skip = true,
                _ => skip_value(&meta)?,
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// Consumes `= value` or `(..)` after a serde key the derive ignores.
fn skip_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _: Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let _ = meta.input.step(|cursor| {
            let mut rest = *cursor;
            if let Some((_, _, next)) = rest.group(proc_macro2::Delimiter::Parenthesis) {
                rest = next;
            }
            Ok(((), rest))
        });
    }
    Ok(())
}

/// Whether `ty` is `Option<..>` (the field may be left empty).
fn is_option(ty: &Type) -> bool {
    match ty {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option"),
        Type::Group(group) => is_option(&group.elem),
        Type::Paren(paren) => is_option(&paren.elem),
        _ => false,
    }
}

fn parse_import_field(field: &syn::Field, serde: &SerdeStruct) -> syn::Result<Option<ImportField>> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| syn::Error::new(field.span(), "expected a named field"))?;
    let serde_args = serde_field(field)?;
    let key = serde_args
        .rename
        .clone()
        .unwrap_or_else(|| field_name(&ident));
    let mut header: Option<LitStr> = None;
    let mut aliases: Vec<LitStr> = Vec::new();
    let mut required: Option<bool> = None;
    let mut skip = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("import") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            match key_name(&meta.path).as_str() {
                "header" => header = Some(non_empty(meta.value()?.parse()?)?),
                "aliases" => {
                    let value: Expr = meta.value()?.parse()?;
                    let Expr::Array(array) = value else {
                        return Err(syn::Error::new(
                            value.span(),
                            "expected a list of strings: aliases = [\"E-mail\", \"Mail\"]",
                        ));
                    };
                    for element in array.elems {
                        match element {
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(text),
                                ..
                            }) => aliases.push(non_empty(text)?),
                            other => {
                                return Err(syn::Error::new(other.span(), "expected a string"));
                            }
                        }
                    }
                }
                "required" => required = Some(true),
                "optional" => required = Some(false),
                "skip" => skip = true,
                other => {
                    return Err(meta.error(format!(
                        "unknown #[import] field argument `{other}`: expected `header = \"..\"`, \
                         `aliases = [..]`, `required`, `optional` or `skip`"
                    )));
                }
            }
            Ok(())
        })?;
    }
    if skip || serde_args.skip {
        return Ok(None);
    }
    let header = header.unwrap_or_else(|| LitStr::new(&sentence_case(&key), ident.span()));
    let required =
        required.unwrap_or_else(|| !(is_option(&field.ty) || serde_args.default || serde.default));
    Ok(Some(ImportField {
        key,
        header,
        aliases,
        required,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Expansion = fn(TokenStream, fn() -> syn::Result<TokenStream>) -> syn::Result<TokenStream>;

    fn sheets() -> syn::Result<TokenStream> {
        Ok(quote!(::navree_sheets))
    }

    fn expand(f: Expansion, tokens: TokenStream) -> String {
        f(tokens, sheets)
            .unwrap_or_else(syn::Error::into_compile_error)
            .to_string()
    }

    #[test]
    fn exportable_lists_columns_and_cells() {
        let out = expand(
            try_expand_exportable,
            quote! {
                #[export(sheet = "Contacts")]
                struct Row {
                    #[export(header = "Name")]
                    name: String,
                    first_name: String,
                    #[export(format = "0.00", width = 12)]
                    amount: f64,
                    #[export(with = "money")]
                    cents: i64,
                    #[export(skip)]
                    secret: String,
                }
            },
        );
        assert!(out.contains("Column :: new (\"name\" , \"Name\")"), "{out}");
        assert!(out.contains("\"first_name\" , \"First name\""), "{out}");
        assert!(out.contains(". format (\"0.00\")"), "{out}");
        assert!(out.contains(". width (12f64)"), "{out}");
        assert!(out.contains("money (& self . cents)"), "{out}");
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("\"Contacts\""), "{out}");
    }

    #[test]
    fn exportable_rejects_unknown_arguments_and_tuple_structs() {
        let out = expand(
            try_expand_exportable,
            quote!(
                struct Row {
                    #[export(colour = "red")]
                    a: String,
                }
            ),
        );
        assert!(
            out.contains("unknown #[export] field argument `colour`"),
            "{out}"
        );
        let out = expand(
            try_expand_exportable,
            quote!(
                struct Row(String);
            ),
        );
        assert!(out.contains("needs a struct with named fields"), "{out}");
        let out = expand(
            try_expand_exportable,
            quote!(
                struct Row {
                    #[export(skip, header = "X")]
                    a: String,
                }
            ),
        );
        assert!(out.contains("a skipped field takes no other"), "{out}");
        let out = expand(
            try_expand_exportable,
            quote!(
                struct Row {
                    #[export(width = 0)]
                    a: String,
                }
            ),
        );
        assert!(out.contains("the width is a number from 1 to 255"), "{out}");
    }

    #[test]
    fn importable_infers_required_columns() {
        let out = expand(
            try_expand_importable,
            quote! {
                struct Row {
                    #[import(header = "Email", aliases = ["E-mail", "Mail"])]
                    email: String,
                    phone: Option<String>,
                    #[serde(default)]
                    tags: Vec<String>,
                    #[serde(rename = "company")]
                    company_name: String,
                    #[import(optional)]
                    note: String,
                    #[serde(skip)]
                    ignored: u8,
                    #[import(skip)]
                    also_ignored: u8,
                }
            },
        );
        assert!(
            out.contains(
                "ImportColumn :: new (\"email\" , \"Email\") . alias (\"E-mail\") . alias (\"Mail\") . required (true)"
            ),
            "{out}"
        );
        assert!(
            out.contains("new (\"phone\" , \"Phone\") . required (false)"),
            "{out}"
        );
        assert!(
            out.contains("new (\"tags\" , \"Tags\") . required (false)"),
            "{out}"
        );
        assert!(
            out.contains("new (\"company\" , \"Company\") . required (true)"),
            "{out}"
        );
        assert!(
            out.contains("new (\"note\" , \"Note\") . required (false)"),
            "{out}"
        );
        assert!(!out.contains("ignored"), "{out}");
    }

    #[test]
    fn importable_rejects_rename_all_and_bad_aliases() {
        let out = expand(
            try_expand_importable,
            quote!(
                #[serde(rename_all = "camelCase")]
                struct Row {
                    a: String,
                }
            ),
        );
        assert!(out.contains("rename fields one by one"), "{out}");
        let out = expand(
            try_expand_importable,
            quote!(
                struct Row {
                    #[import(aliases = "x")]
                    a: String,
                }
            ),
        );
        assert!(out.contains("expected a list of strings"), "{out}");
        let out = expand(
            try_expand_importable,
            quote!(
                struct Row {
                    #[import(skip)]
                    a: String,
                }
            ),
        );
        assert!(out.contains("at least one field"), "{out}");
        let out = expand(
            try_expand_importable,
            quote!(
                struct Row {
                    #[serde(rename = "a")]
                    b: String,
                    a: String,
                }
            ),
        );
        assert!(out.contains("two fields read the column `a`"), "{out}");
    }

    #[test]
    fn a_struct_wide_serde_default_makes_every_field_optional() {
        let out = expand(
            try_expand_importable,
            quote!(
                #[serde(default, deny_unknown_fields)]
                struct Row {
                    a: String,
                }
            ),
        );
        assert!(out.contains("required (false)"), "{out}");
    }
}
