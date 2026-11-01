//! `#[derive(TableRow)]` and `#[derive(FilterBar)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{DeriveInput, LitInt, LitStr, Path};

use crate::shared::{apply_case, doc_line, named_fields, option_inner, serde_rename, serde_rename_all, ui_path, words};

#[derive(Default)]
struct ColHints {
    skip: bool,
    key: bool,
    sortable: bool,
    numeric: bool,
    hide_on_mobile: bool,
    search: bool,
    strong: bool,
    sticky: bool,
    sticky_end: bool,
    hidden: bool,
    truncate: bool,
    wrap: bool,
    locked: bool,
    width: Option<LitInt>,
    align: Option<syn::Ident>,
    kind: Option<syn::Ident>,
    filter: Option<syn::Ident>,
    options: Option<Path>,
    source: Option<LitStr>,
    group: Option<LitStr>,
    label: Option<LitStr>,
    link: Option<LitStr>,
    badge: Option<Path>,
}

fn col_hints(attrs: &[syn::Attribute]) -> syn::Result<ColHints> {
    let mut out = ColHints::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("col")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            match key.as_str() {
                "skip" => out.skip = true,
                "key" => out.key = true,
                "sortable" => out.sortable = true,
                "numeric" => out.numeric = true,
                "hide_on_mobile" => out.hide_on_mobile = true,
                "search" => out.search = true,
                "strong" => out.strong = true,
                "sticky" => out.sticky = true,
                "sticky_end" => out.sticky_end = true,
                "hidden" => out.hidden = true,
                "truncate" => out.truncate = true,
                "wrap" => out.wrap = true,
                "locked" => out.locked = true,
                "width" => out.width = Some(meta.value()?.parse()?),
                "align" => out.align = Some(meta.value()?.parse()?),
                "kind" => out.kind = Some(meta.value()?.parse()?),
                "filter" => out.filter = Some(meta.value()?.parse()?),
                "options" => out.options = Some(meta.value()?.parse()?),
                "source" => out.source = Some(meta.value()?.parse()?),
                "group" => out.group = Some(meta.value()?.parse()?),
                "label" => out.label = Some(meta.value()?.parse()?),
                "link" => out.link = Some(meta.value()?.parse()?),
                "badge" => out.badge = Some(meta.value()?.parse()?),
                _ => {
                    return Err(meta.error(format!(
                        "unknown `col` option `{key}`: expected skip, key, sortable, numeric, hide_on_mobile, search, strong, sticky, sticky_end, hidden, truncate, wrap, locked, width, align, kind, filter, options, source, group, label, link or badge"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

#[derive(Default)]
struct TableHints {
    sort: Option<LitStr>,
    per: Option<LitInt>,
    per_pages: Vec<LitInt>,
    cursor: bool,
    version: Option<LitStr>,
    no_count: bool,
    capped: Option<LitInt>,
}

fn table_hints(attrs: &[syn::Attribute]) -> syn::Result<TableHints> {
    let mut out = TableHints::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("table")) {
        attr.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().map(ToString::to_string).unwrap_or_default();
            match key.as_str() {
                "sort" => out.sort = Some(meta.value()?.parse()?),
                "per" => out.per = Some(meta.value()?.parse()?),
                "per_pages" => {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let list = content.parse_terminated(<LitInt as syn::parse::Parse>::parse, syn::Token![,])?;
                    out.per_pages = list.into_iter().collect();
                }
                "cursor" => out.cursor = true,
                "version" => out.version = Some(meta.value()?.parse()?),
                "no_count" => out.no_count = true,
                "capped" => out.capped = Some(meta.value()?.parse()?),
                _ => {
                    return Err(meta.error(format!("unknown `table` option `{key}`: expected sort, per, per_pages, cursor, version, no_count or capped")));
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// `-updated_at,title` as `(name, direction)` pairs, checked against the columns.
fn sort_terms(sort: &LitStr, ui: &TokenStream, columns: &[String]) -> syn::Result<Vec<TokenStream>> {
    let mut out = Vec::new();
    for term in sort.value().split(',').map(str::trim).filter(|term| !term.is_empty()) {
        let (name, direction) = match term.strip_prefix('-') {
            Some(name) => (name, quote!(#ui::table::Direction::Desc)),
            None => (term, quote!(#ui::table::Direction::Asc)),
        };
        if !columns.iter().any(|column| column == name) {
            return Err(syn::Error::new(sort.span(), format!("`{name}` in `sort` is not a column")));
        }
        out.push(quote!((#name, #direction)));
    }
    Ok(out)
}

/// Turns `/deals/{id}` into `format!("/deals/{}", path_segment(&self.id))`, naming
/// fields only.
fn link_expr(link: &LitStr, ui: &TokenStream) -> syn::Result<TokenStream> {
    let text = link.value();
    let mut pattern = String::new();
    let mut args = Vec::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            return Err(syn::Error::new(link.span(), "`link` has a `{` with no `}`"));
        };
        pattern.push_str(&rest[..open]);
        pattern.push_str("{}");
        let name = &rest[open + 1..open + close];
        let field = syn::parse_str::<syn::Ident>(name).map_err(|_| syn::Error::new(link.span(), format!("`{{{name}}}` in `link` is not a field name")))?;
        args.push(quote!(#ui::table::path_segment(&self.#field)));
        rest = &rest[open + close + 1..];
    }
    pattern.push_str(rest);
    Ok(quote!(::std::format!(#pattern, #(#args),*)))
}

fn option_tokens<T: quote::ToTokens>(value: Option<&T>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

pub(crate) fn table_row(input: DeriveInput) -> syn::Result<TokenStream> {
    let ui = ui_path(input.ident.span())?;
    let fields = named_fields(&input, "TableRow")?;
    let table = table_hints(&input.attrs)?;
    let mut columns = Vec::new();
    let mut cells = Vec::new();
    let mut texts = Vec::new();
    let mut names = Vec::new();
    let mut key = quote!(::core::option::Option::None);
    let mut key_kind = quote!(#ui::table::ValueKind::Int);
    for field in &fields.named {
        let hint = col_hints(&field.attrs)?;
        let Some(ident) = &field.ident else { continue };
        let name = ident.unraw().to_string();
        let ty = &field.ty;
        // A field called `id`, or marked `key`, keeps its row through a morph.
        if hint.key || name == "id" {
            key = quote!(::core::option::Option::Some(::std::string::ToString::to_string(&self.#ident)));
            key_kind = quote!(<#ty as #ui::table::ColumnValue>::KIND);
        }
        if hint.skip {
            continue;
        }
        let index = columns.len();
        names.push(name.clone());
        let label = hint.label.as_ref().map(LitStr::value).or_else(|| doc_line(&field.attrs)).unwrap_or_else(|| words(&name));
        let source = hint.source.as_ref().map_or_else(|| name.clone(), LitStr::value);
        let (sortable, numeric, hide, search, strong) = (hint.sortable, hint.numeric, hint.hide_on_mobile, hint.search, hint.strong);
        let (hidden, truncate, wrap, locked) = (hint.hidden, hint.truncate, hint.wrap, hint.locked);
        if hint.sticky && hint.sticky_end {
            return Err(syn::Error::new_spanned(ident, "a column is `sticky` or `sticky_end`, not both"));
        }
        let sticky = match (hint.sticky, hint.sticky_end) {
            (true, _) => quote!(#ui::table::Sticky::Start),
            (_, true) => quote!(#ui::table::Sticky::End),
            _ => quote!(#ui::table::Sticky::No),
        };
        let align = match hint.align.as_ref().map(ToString::to_string).as_deref() {
            None | Some("start") => quote!(#ui::table::Align::Start),
            Some("center") => quote!(#ui::table::Align::Center),
            Some("end") => quote!(#ui::table::Align::End),
            Some(other) => return Err(syn::Error::new_spanned(&hint.align, format!("`align = {other}`: expected start, center or end"))),
        };
        let kind = match hint.kind.as_ref().map(ToString::to_string).as_deref() {
            None => quote!(<#ty as #ui::table::ColumnValue>::KIND),
            Some("text") => quote!(#ui::table::ValueKind::Text),
            Some("int") => quote!(#ui::table::ValueKind::Int),
            Some("money") => quote!(#ui::table::ValueKind::Money),
            Some("float") => quote!(#ui::table::ValueKind::Float),
            Some("decimal") => quote!(#ui::table::ValueKind::Decimal),
            Some("date") => quote!(#ui::table::ValueKind::Date),
            Some("datetime") => quote!(#ui::table::ValueKind::DateTime),
            Some("bool") => quote!(#ui::table::ValueKind::Bool),
            Some("uuid") => quote!(#ui::table::ValueKind::Uuid),
            Some(other) => return Err(syn::Error::new_spanned(&hint.kind, format!("`kind = {other}`: expected text, int, money, float, decimal, date, datetime, bool or uuid"))),
        };
        let width = option_tokens(hint.width.as_ref());
        let group = option_tokens(hint.group.as_ref());
        let filter_name = hint.filter.as_ref().map(ToString::to_string);
        let filter = match filter_name.as_deref() {
            None => quote!(#ui::table::FilterKind::None),
            Some("text") => quote!(#ui::table::FilterKind::Text),
            Some("range" | "date") => quote!(#ui::table::FilterKind::Range),
            Some("select") => quote!(#ui::table::FilterKind::Select),
            Some("multi") => quote!(#ui::table::FilterKind::Multi),
            Some("boolean") => quote!(#ui::table::FilterKind::Boolean),
            Some("relation") => quote!(#ui::table::FilterKind::Relation),
            Some(other) => {
                return Err(syn::Error::new_spanned(&hint.filter, format!("`filter = {other}`: expected text, range, date, select, multi, boolean or relation")));
            }
        };
        let options = match (&hint.options, filter_name.as_deref()) {
            (Some(path), _) => quote!(#path),
            (None, Some("select" | "multi")) => {
                let inner = option_inner(ty).unwrap_or(ty);
                quote!(<#inner as #ui::form::Options>::OPTIONS)
            }
            _ => quote!(&[]),
        };
        columns.push(quote! {
            #ui::table::Column {
                name: #name,
                label: #label,
                source: #source,
                kind: #kind,
                nullable: <#ty as #ui::table::ColumnValue>::NULLABLE,
                sortable: #sortable,
                numeric: #numeric,
                hide_on_mobile: #hide,
                search: #search,
                strong: #strong,
                hidden: #hidden,
                truncate: #truncate,
                wrap: #wrap,
                locked: #locked,
                sticky: #sticky,
                align: #align,
                width: #width,
                group: #group,
                filter: #filter,
                options: #options,
            },
        });
        // A field with its own list of options shows the option's label.
        let shown = match &hint.options {
            Some(path) => quote!(#ui::table::OptionCell { value: &self.#ident, options: #path }),
            None => quote!(self.#ident),
        };
        let content = match (&hint.link, &hint.badge) {
            (Some(link), _) => {
                let href = link_expr(link, &ui)?;
                quote!(#ui::table::write_link(out, #href, &#shown);)
            }
            (None, Some(tone)) => quote!(#ui::table::write_badge(out, &#shown, #tone(&self.#ident));),
            (None, None) => quote!(#ui::table::CellValue::write_cell(&#shown, out);),
        };
        cells.push(quote!(#index => { #content }));
        texts.push(quote!(#index => #ui::table::CellValue::write_text(&#shown, out),));
    }
    let sort = match &table.sort {
        Some(sort) => sort_terms(sort, &ui, &names)?,
        None => Vec::new(),
    };
    let per = table.per.as_ref().map_or_else(|| quote!(25), |per| quote!(#per));
    let per_pages = if table.per_pages.is_empty() {
        quote!(&[10, 25, 50, 100])
    } else {
        let list = &table.per_pages;
        quote!(&[#(#list),*])
    };
    let cursor = table.cursor;
    let version = option_tokens(table.version.as_ref());
    let count = match (table.no_count, &table.capped) {
        (true, _) => quote!(#ui::table::Count::None),
        (false, Some(cap)) => quote!(#ui::table::Count::Capped(#cap)),
        (false, None) => quote!(#ui::table::Count::Exact),
    };
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ui::table::TableRow for #ident #ty_generics #where_clause {
            const COLUMNS: &'static [#ui::table::Column] = &[#(#columns)*];

            const CONFIG: #ui::table::TableConfig = #ui::table::TableConfig {
                key_kind: #key_kind,
                sort: &[#(#sort),*],
                per: #per,
                per_pages: #per_pages,
                cursor: #cursor,
                version: #version,
                count: #count,
            };

            fn key(&self) -> ::core::option::Option<::std::string::String> {
                #key
            }

            fn write_cell(&self, column: usize, out: &mut #ui::table::CellWriter<'_>) {
                match column {
                    #(#cells)*
                    _ => {}
                }
            }

            fn write_text(&self, column: usize, out: &mut ::std::string::String) {
                match column {
                    #(#texts)*
                    _ => {}
                }
            }
        }
    })
}

pub(crate) fn filter_bar(input: DeriveInput) -> syn::Result<TokenStream> {
    let ui = ui_path(input.ident.span())?;
    let fields = named_fields(&input, "FilterBar")?;
    let case = serde_rename_all(&input.attrs);
    let mut controls = Vec::new();
    let mut params = Vec::new();
    for field in &fields.named {
        let Some(ident) = &field.ident else { continue };
        let rust_name = ident.unraw().to_string();
        let name = serde_rename(&field.attrs).unwrap_or_else(|| apply_case(&rust_name, case.as_deref()));
        let label = doc_line(&field.attrs).unwrap_or_else(|| words(&rust_name));
        let mut search = false;
        for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("filter")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("search") {
                    search = true;
                    Ok(())
                } else {
                    Err(meta.error("unknown `filter` option: expected `search`"))
                }
            })?;
        }
        let Some(inner) = option_inner(&field.ty) else {
            return Err(syn::Error::new_spanned(&field.ty, "a filter is optional: write `Option<...>`"));
        };
        let value = format_ident!("__value");
        params.push(quote! {
            if let ::core::option::Option::Some(#value) = #ui::form::FieldValue::field_value(&self.#ident) {
                out.push((#name, #value));
            }
        });
        controls.push(if search {
            quote!(out.push(#ui::table::search_control(#name, #label, #ui::form::FieldValue::field_value(&self.#ident)));)
        } else {
            quote!(out.push(#ui::table::select_control(#name, #label, <#inner as #ui::form::Options>::OPTIONS, #ui::form::FieldValue::field_value(&self.#ident)));)
        });
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ui::table::FilterBar for #ident #ty_generics #where_clause {
            fn params(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                let mut out = ::std::vec::Vec::new();
                #(#params)*
                out
            }

            fn controls(&self) -> ::std::vec::Vec<#ui::Node> {
                let mut out = ::std::vec::Vec::new();
                #(#controls)*
                out
            }
        }
    })
}
