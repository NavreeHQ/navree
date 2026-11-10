//! # navree-validation-macros
//!
//! `#[derive(Validate)]`, re-exported by navree-validation (and through it
//! by the `navree` facade as `navree::validation::Validate`): depend on
//! `navree-validation` or `navree`, not on this crate.
//!
//! The expansion names the runtime crate as `::navree_validation` when the
//! crate being compiled depends on navree-validation, else as
//! `::navree::validation` through the facade (honouring a renamed
//! dependency); `#[validate(crate = "...")]` names it explicitly.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

use proc_macro::TokenStream;

mod expand;
mod paths;
mod rules;
mod serde_names;

/// Implements `Validate` from the `#[validate(...)]` rules of each field.
///
/// Every field states its rules, or `skip`; `#[validate(skip_unmarked)]`
/// on the type lets fields without `#[validate]` go unchecked. The rules
/// are listed in navree-validation's documentation (`length`, `range`,
/// `required`, `email`, `url`, `pattern`, `custom`, `nested`, `each`, ...).
///
/// ```ignore
/// #[derive(Deserialize, Validate)]
/// struct CreateContact {
///     #[validate(length(min = 1, max = 100))]
///     name: String,
///     #[validate(email)]
///     email: Option<String>,
///     #[validate(length(max = 10), each(length(min = 1, max = 30)))]
///     tags: Vec<String>,
///     #[validate(nested)]
///     address: Address,
///     #[validate(skip)]
///     source: Source,
/// }
/// ```
#[proc_macro_derive(Validate, attributes(validate))]
pub fn derive_validate(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
