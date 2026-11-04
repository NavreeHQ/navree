//! # navree-ui-macros
//!
//! The derives and icon macros of navree-ui, re-exported by it (and through it by the `navree`
//! facade as `navree::ui`): depend on `navree-ui` or `navree`, not on this crate.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod form;
mod icon;
mod shared;
mod table;

/// Implements `Form` from a struct's fields: the type picks the widget (`bool`
/// a checkbox, a number a number input, an enum with `Options` a select), the
/// doc comment is the label, `Option` means optional, and the rules come from
/// `#[derive(Validate)]`. A type that implements navree-validation's
/// `AsyncRules` (`unique`, `exists`) gets them as its `database_rules`, which
/// `Submit` runs once the rules pass (a generic type doesn't).
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
/// time | datetime | date_picker | time_picker | textarea | select | radio | checkbox |
/// switch | hidden | money`, and the flags `autofocus`, `wide` and `skip`.
#[proc_macro_derive(Form, attributes(form))]
pub fn derive_form(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    form::form(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements `Options` for an enum of unit variants, so a field of this type
/// is a select. The label is `#[option(label = "...")]`, the doc comment, or
/// the variant's name in words; the value is the name serde reads.
#[proc_macro_derive(Options, attributes(option))]
pub fn derive_options(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    form::options(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
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
/// "..."`, `link = "/deals/{id}"`, `badge = path::to::tone`, `cell = path::to::draw`
/// (draws the cell from the whole row, into anything that becomes a `Node`).
///
/// On the struct: `#[table(sort = "-updated_at,title", per = 25, per_pages(10, 25,
/// 50, 100), cursor, version = "updated_at", no_count, capped = 10000)]`.
#[proc_macro_derive(TableRow, attributes(col, table))]
pub fn derive_table_row(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    table::table_row(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Implements `FilterBar` for a struct: each field is a control of the table's
/// toolbar, and a parameter in the URL. `#[filter(search)]` is a search box;
/// an `Option` of an enum with `Options` is a select.
#[proc_macro_derive(FilterBar, attributes(filter))]
pub fn derive_filter_bar(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    table::filter_bar(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Your own SVG file as an `Icon`, checked while the app compiles. The path is
/// relative to your crate's `Cargo.toml`.
///
/// ```ignore
/// const LOGO: Icon = icon_file!("assets/icons/logo.svg");
/// Button::new("Home").icon(LOGO)
/// ```
///
/// Only shape elements (`path`, `circle`, `ellipse`, `rect`, `line`, `polyline`,
/// `polygon`, `g`) and plain drawing attributes survive. Scripts, styles, links,
/// `use`, images, gradients, event handlers and anything else stop the build with
/// the file, the line and the reason. A shape with no paint is drawn in the text
/// colour, so the icon follows light and dark themes; `fill` and `stroke` take
/// `none`, `currentColor` or a hex colour. Cargo rebuilds when the file changes.
#[proc_macro]
pub fn icon_file(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as syn::LitStr);
    expand(&path, icon::file)
}

/// Every `.svg` file in a directory as constants, in the module where it is
/// written. The directory is relative to your crate's `Cargo.toml`.
///
/// ```ignore
/// mod brand {
///     navree::ui::icon_set!("assets/icons");
/// }
/// // assets/icons/logo.svg is brand::LOGO; brand::ALL lists them all.
/// Button::new("Home").icon(brand::LOGO)
/// ```
///
/// Each file is checked as `icon_file!` checks it. A file name becomes a constant
/// name: `arrow-left.svg` is `ARROW_LEFT`. Cargo notices a file that changes but
/// not one that is added: add `println!("cargo::rerun-if-changed=assets/icons");`
/// to a `build.rs`, or touch the file that calls this macro.
#[proc_macro]
pub fn icon_set(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as syn::LitStr);
    expand(&path, icon::set)
}

/// Runs an icon macro with the paths it needs.
fn expand(
    path: &syn::LitStr,
    make: fn(
        &proc_macro2::TokenStream,
        &std::path::Path,
        &syn::LitStr,
    ) -> syn::Result<proc_macro2::TokenStream>,
) -> TokenStream {
    let result = shared::ui_path(path.span()).and_then(|ui| {
        let manifest = icon::manifest_dir(path.span())?;
        make(&ui, &manifest, path)
    });
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
