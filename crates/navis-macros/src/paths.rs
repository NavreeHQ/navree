//! Paths to the runtime crates the expansions call into.

use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

/// A runtime crate the generated code needs, and where the `navis` facade
/// re-exports it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeCrate {
    /// The package name, e.g. `navis-kernel`.
    pub(crate) package: &'static str,
    /// The facade module re-exporting it, e.g. `kernel` for `navis::kernel`.
    pub(crate) facade_module: &'static str,
}

/// navis-kernel: `Module`, `ModuleBuilder`, `Injectable`, `Resolver`, ...
pub(crate) const KERNEL: RuntimeCrate = RuntimeCrate {
    package: "navis-kernel",
    facade_module: "kernel",
};

/// navis-http: `ModuleBuilderHttpExt` (controllers).
pub(crate) const HTTP: RuntimeCrate = RuntimeCrate {
    package: "navis-http",
    facade_module: "http",
};

/// navis-access: `ActionMarker` and `Subject` (`#[can(Update, Deal)]`).
pub(crate) const ACCESS: RuntimeCrate = RuntimeCrate {
    package: "navis-access",
    facade_module: "access",
};

/// The facade package, preferred over the individual crates.
const FACADE: &str = "navis";

impl RuntimeCrate {
    /// The path to this crate as seen from the crate being compiled
    /// (`::navis::kernel`, `::navis_kernel`, or `crate` inside the crate itself).
    ///
    /// Fails, pointing at `span`, when the crate being compiled depends on
    /// neither the facade nor this crate.
    pub(crate) fn path(self, span: Span) -> syn::Result<TokenStream> {
        resolve(self, |package| proc_macro_crate::crate_name(package).ok()).ok_or_else(|| {
            syn::Error::new(
                span,
                format!(
                    "this attribute needs the `{FACADE}` crate (or `{}`) in the [dependencies] of your Cargo.toml",
                    self.package
                ),
            )
        })
    }
}

/// Resolves the path to `krate` with `lookup` (a `Cargo.toml` dependency
/// lookup by package name): the `navis` facade first, then the crate itself.
fn resolve(
    krate: RuntimeCrate,
    lookup: impl Fn(&str) -> Option<FoundCrate>,
) -> Option<TokenStream> {
    let module = format_ident!("{}", krate.facade_module);
    if let Some(facade) = lookup(FACADE) {
        let root = crate_root(facade);
        return Some(quote!(#root::#module));
    }
    lookup(krate.package).map(crate_root)
}

/// `crate` for the crate being compiled, `::name` for a dependency.
fn crate_root(found: FoundCrate) -> TokenStream {
    match found {
        FoundCrate::Itself => quote!(crate),
        FoundCrate::Name(name) => {
            let name = format_ident!("{name}");
            quote!(::#name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(found: &[(&str, FoundCrate)]) -> Option<String> {
        resolve(KERNEL, |name| {
            found
                .iter()
                .find(|(package, _)| *package == name)
                .map(|(_, f)| f.clone())
        })
        .map(|tokens| tokens.to_string())
    }

    #[test]
    fn prefers_the_facade() {
        let found = [
            ("navis", FoundCrate::Name("navis".into())),
            ("navis-kernel", FoundCrate::Name("navis_kernel".into())),
        ];
        assert_eq!(path(&found).as_deref(), Some(":: navis :: kernel"));
    }

    #[test]
    fn honours_a_renamed_facade() {
        let found = [("navis", FoundCrate::Name("framework".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: framework :: kernel"));
    }

    #[test]
    fn inside_the_facade_uses_crate() {
        let found = [("navis", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some("crate :: kernel"));
    }

    #[test]
    fn falls_back_to_the_runtime_crate() {
        let found = [("navis-kernel", FoundCrate::Name("navis_kernel".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: navis_kernel"));
    }

    #[test]
    fn inside_the_runtime_crate_uses_crate() {
        let found = [("navis-kernel", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some("crate"));
    }

    #[test]
    fn none_when_neither_is_a_dependency() {
        assert_eq!(path(&[]), None);
    }
}
