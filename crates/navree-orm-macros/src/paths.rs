//! The path to the ORM runtime crate as the crate being compiled sees it.

use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

/// The runtime crate's package.
const ORM: &str = "navree-orm";

/// The facade package, which re-exports the runtime crate as `orm`.
const FACADE: &str = "navree";

/// The path to navree-orm: `::navree_orm` when the crate being compiled
/// depends on it (or is it: navree-orm names itself through `extern crate
/// self as navree_orm`), else `::navree::orm` through the facade.
pub(crate) fn orm(span: Span) -> syn::Result<TokenStream> {
    resolve(|package| proc_macro_crate::crate_name(package).ok()).ok_or_else(|| {
        syn::Error::new(
            span,
            format!(
                "this macro needs the `{FACADE}` crate (or `{ORM}`) in the [dependencies] of your Cargo.toml"
            ),
        )
    })
}

/// Resolves the path with `lookup` (a dependency lookup by package name).
fn resolve(lookup: impl Fn(&str) -> Option<FoundCrate>) -> Option<TokenStream> {
    if let Some(found) = lookup(ORM) {
        return Some(match found {
            FoundCrate::Itself => quote!(::navree_orm),
            FoundCrate::Name(name) => {
                let name = format_ident!("{name}");
                quote!(::#name)
            }
        });
    }
    lookup(FACADE).map(|found| match found {
        // Inside the facade's own tests, which can name navree-orm directly.
        FoundCrate::Itself => quote!(::navree_orm),
        FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name::orm)
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
            ("navree-orm", FoundCrate::Name("navree_orm".into())),
        ];
        assert_eq!(path(&found).as_deref(), Some(":: navree_orm"));
    }

    #[test]
    fn inside_the_runtime_crate_names_itself() {
        let found = [("navree-orm", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some(":: navree_orm"));
    }

    #[test]
    fn goes_through_the_facade_honouring_a_rename() {
        let found = [("navree", FoundCrate::Name("framework".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: framework :: orm"));
    }

    #[test]
    fn none_without_either_dependency() {
        assert_eq!(path(&[]), None);
    }
}
