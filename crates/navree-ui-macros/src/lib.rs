//! # navree-ui-macros
//!
//! The derives of navree-ui, re-exported by it (and through it by the `navree`
//! facade as `navree::ui`): depend on `navree-ui` or `navree`, not on this crate.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod form;
mod shared;
mod table;

/// Implements `Form` from a struct's fields: the type picks the widget (`bool`
/// a checkbox, a number a number input, an enum with `Options` a select), the
/// doc comment is the label, `Option` means optional, and the rules come from
/// `#[derive(Validate)]`.
///
/// ```ignore
/// #[derive(Deserialize, Validate, Form)]
/// struct NewDeal {
///     /// Deal name
///     #[validate(length(min = 1, max = 255))]
///     #[form(placeholder = "Annual platform license", autofocus, wide)]
///     title: String,
/// }
/// ```
///
/// On the type: `id = "..."`. On a field: `label`, `placeholder`, `help`,
/// `prompt` and `error` (text), `rows` (a number), `addon = "other_field"`,
/// `widget = text | email | url | tel | password | search | number | date |
/// time | datetime | textarea | select | radio | checkbox | switch | hidden |
/// money`, and the flags `autofocus`, `wide` and `skip`.
#[proc_macro_derive(Form, attributes(form))]
pub fn derive_form(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    form::form(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Implements `Options` for an enum of unit variants, so a field of this type
/// is a select. The label is `#[option(label = "...")]`, the doc comment, or
/// the variant's name in words; the value is the name serde reads.
#[proc_macro_derive(Options, attributes(option))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    form::options(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Implements `TableRow` for a struct: each field is a column.
///
/// On a field: `#[col(skip)]`, `key`, `sortable`, `numeric`, `hide_on_mobile`,
/// `search` (in the global search), `strong`, `sticky`, `sticky_end`, `hidden`
/// (off until shown), `truncate`, `wrap`, `locked` (can't be hidden), `width =
/// 240`, `align = start | center | end`, `kind = int` (how filter values bind, when
/// the cell shows something other than the column queried), `filter = text | range | date | select |
/// multi | boolean | relation`, `options = PATH`, `source = "companies.name"` (the
/// column queried when it isn't the field's name), `group = "Address"`, `label =
/// "..."`, `link = "/deals/{id}"`, `badge = path::to::tone`.
///
/// On the struct: `#[table(sort = "-updated_at,title", per = 25, per_pages(10, 25,
/// 50, 100), cursor, version = "updated_at", no_count, capped = 10000)]`.
#[proc_macro_derive(TableRow, attributes(col, table))]
pub fn derive_table_row(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    table::table_row(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Implements `FilterBar` for a struct: each field is a control of the table's
/// toolbar, and a parameter in the URL. `#[filter(search)]` is a search box;
/// an `Option` of an enum with `Options` is a select.
#[proc_macro_derive(FilterBar, attributes(filter))]
pub fn derive_filter_bar(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    table::filter_bar(input).unwrap_or_else(syn::Error::into_compile_error).into()
}
