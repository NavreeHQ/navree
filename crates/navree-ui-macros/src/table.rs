//! `#[derive(TableRow)]` and `#[derive(FilterBar)]`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::{DeriveInput, LitStr, Path};

use crate::shared::{apply_case, doc_line, named_fields, option_inner, serde_rename, serde_rename_all, ui_path, words};

#[derive(Default)]
struct ColHints {
    skip: bool,
    key: bool,
    sortable: bool,
    numeric: bool,
    hide_on_mobile: bool,
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
                "label" => out.label = Some(meta.value()?.parse()?),
                "link" => out.link = Some(meta.value()?.parse()?),
                "badge" => out.badge = Some(meta.value()?.parse()?),
                _ => {
                    return Err(meta.error(format!(
                        "unknown `col` option `{key}`: expected skip, key, sortable, numeric, hide_on_mobile, label, link or badge"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(out)
}

/// Turns `/deals/{id}` into `format!("/deals/{}", self.id)`, naming fields only.
fn link_expr(link: &LitStr) -> syn::Result<TokenStream> {
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
        args.push(quote!(self.#field));
        rest = &rest[open + close + 1..];
    }
    pattern.push_str(rest);
    Ok(quote!(::std::format!(#pattern, #(#args),*)))
}

pub(crate) fn table_row(input: DeriveInput) -> syn::Result<TokenStream> {
    let ui = ui_path(input.ident.span())?;
    let fields = named_fields(&input, "TableRow")?;
    let mut columns = Vec::new();
    let mut cells = Vec::new();
    let mut key = quote!(::core::option::Option::None);
    for field in &fields.named {
        let hint = col_hints(&field.attrs)?;
        let Some(ident) = &field.ident else { continue };
        let name = ident.unraw().to_string();
        // A field called `id`, or marked `key`, keeps its row through a morph.
        if hint.key || name == "id" {
            key = quote!(::core::option::Option::Some(::std::string::ToString::to_string(&self.#ident)));
        }
        if hint.skip {
            continue;
        }
        let label = hint.label.as_ref().map(LitStr::value).or_else(|| doc_line(&field.attrs)).unwrap_or_else(|| words(&name));
        let (sortable, numeric, hide) = (hint.sortable, hint.numeric, hint.hide_on_mobile);
        columns.push(quote! {
            #ui::table::Column { name: #name, label: #label, sortable: #sortable, numeric: #numeric, hide_on_mobile: #hide },
        });
        let content = match (&hint.link, &hint.badge) {
            (Some(link), _) => {
                let href = link_expr(link)?;
                quote!(#ui::table::write_link(out, #href, &self.#ident);)
            }
            (None, Some(tone)) => quote!(#ui::table::write_badge(out, &self.#ident, #tone(&self.#ident));),
            (None, None) => quote!(#ui::table::CellValue::write_cell(&self.#ident, out);),
        };
        let open = match (numeric, hide) {
            (false, false) => "<td>",
            (true, false) => "<td data-numeric>",
            (false, true) => "<td data-hide-mobile>",
            (true, true) => "<td data-numeric data-hide-mobile>",
        };
        cells.push(quote! {
            out.push_str(#open);
            #content
            out.push_str("</td>");
        });
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ui::table::TableRow for #ident #ty_generics #where_clause {
            const COLUMNS: &'static [#ui::table::Column] = &[#(#columns)*];

            fn key(&self) -> ::core::option::Option<::std::string::String> {
                #key
            }

            fn write_cells(&self, out: &mut ::std::string::String) {
                #(#cells)*
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
