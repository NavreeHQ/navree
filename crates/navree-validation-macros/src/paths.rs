//! The path to the validation runtime crate as the crate being compiled
//! sees it.

use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

/// The runtime crate's package.
const VALIDATION: &str = "navree-validation";

/// The facade package, which re-exports the runtime crate as `validation`.
const FACADE: &str = "navree";

/// The path to navree-validation: `::navree_validation` when the crate
/// being compiled depends on it (or is it: navree-validation names itself
/// through `extern crate self as navree_validation`), else
/// `::navree::validation` through the facade.
pub(crate) fn validation(span: Span) -> syn::Result<TokenStream> {
    resolve(|package| proc_macro_crate::crate_name(package).ok()).ok_or_else(|| {
        syn::Error::new(
            span,
            format!(
                "`#[derive(Validate)]` needs the `{FACADE}` crate (or `{VALIDATION}`) in the [dependencies] of your Cargo.toml, or `#[validate(crate = \"path::to::validation\")]`"
            ),
        )
    })
}

/// Resolves the path with `lookup` (a dependency lookup by package name).
fn resolve(lookup: impl Fn(&str) -> Option<FoundCrate>) -> Option<TokenStream> {
    if let Some(found) = lookup(VALIDATION) {
        return Some(match found {
            FoundCrate::Itself => quote!(::navree_validation),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name)
            }
        });
    }
    lookup(FACADE).map(|found| match found {
        // Inside the facade's own tests, which can name navree-validation directly.
        FoundCrate::Itself => quote!(::navree_validation),
        FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name::validation)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(found: &[(&str, FoundCrate)]) -> Option<String> {
        resolve(|name| {
            found
                .iter()
                .find(|(package, _)| *package == name)
                .map(|(_, f)| f.clone())
        })
        .map(|tokens| tokens.to_string())
    }

    #[test]
    fn prefers_the_runtime_crate() {
        let found = [
            ("navree", FoundCrate::Name("navree".into())),
            (
                "navree-validation",
                FoundCrate::Name("navree_validation".into()),
            ),
        ];
        assert_eq!(path(&found).as_deref(), Some(":: navree_validation"));
    }

    #[test]
    fn inside_the_runtime_crate_names_itself() {
        let found = [("navree-validation", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some(":: navree_validation"));
    }

    #[test]
    fn goes_through_the_facade_honouring_a_rename() {
        let found = [("navree", FoundCrate::Name("framework".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: framework :: validation"));
    }

    #[test]
    fn none_without_either_dependency() {
        assert_eq!(path(&[]), None);
    }
}
