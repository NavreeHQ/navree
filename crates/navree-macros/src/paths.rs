//! Paths to the runtime crates the expansions call into.

use proc_macro_crate::FoundCrate;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

/// A runtime crate the generated code needs, and where the `navree` facade
/// re-exports it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeCrate {
    /// The package name, e.g. `navree-kernel`.
    pub(crate) package: &'static str,
    /// The facade module re-exporting it, e.g. `kernel` for `navree::kernel`.
    pub(crate) facade_module: &'static str,
}

/// navree-kernel: `Module`, `ModuleBuilder`, `Injectable`, `Resolver`, ...
pub(crate) const KERNEL: RuntimeCrate = RuntimeCrate {
    package: "navree-kernel",
    facade_module: "kernel",
};

/// navree-http: `ModuleBuilderHttpExt` (controllers).
pub(crate) const HTTP: RuntimeCrate = RuntimeCrate {
    package: "navree-http",
    facade_module: "http",
};

/// navree-access: `ActionMarker` and `Subject` (`#[can(Update, Deal)]`).
pub(crate) const ACCESS: RuntimeCrate = RuntimeCrate {
    package: "navree-access",
    facade_module: "access",
};

/// navree-flags: `Flag` (`#[flag]`) and `requires_flag`
/// (`#[requires_flag(NewPipeline)]`).
pub(crate) const FLAGS: RuntimeCrate = RuntimeCrate {
    package: "navree-flags",
    facade_module: "flags",
};

/// navree-graphql: `GraphqlResolver`, async-graphql and the field runtime
/// (`#[resolver]`).
pub(crate) const GRAPHQL: RuntimeCrate = RuntimeCrate {
    package: "navree-graphql",
    facade_module: "graphql",
};

/// navree-grpc: the call pipeline of `#[grpc_service]` methods.
pub(crate) const GRPC: RuntimeCrate = RuntimeCrate {
    package: "navree-grpc",
    facade_module: "grpc",
};

/// The facade package, preferred over the individual crates.
const FACADE: &str = "navree";

impl RuntimeCrate {
    /// The path to this crate as seen from the crate being compiled
    /// (`::navree::kernel`, `::navree_kernel`, or `crate` inside the crate itself).
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
/// lookup by package name): the `navree` facade first, then the crate itself.
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
            ("navree", FoundCrate::Name("navree".into())),
            ("navree-kernel", FoundCrate::Name("navree_kernel".into())),
        ];
        assert_eq!(path(&found).as_deref(), Some(":: navree :: kernel"));
    }

    #[test]
    fn honours_a_renamed_facade() {
        let found = [("navree", FoundCrate::Name("framework".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: framework :: kernel"));
    }

    #[test]
    fn inside_the_facade_uses_crate() {
        let found = [("navree", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some("crate :: kernel"));
    }

    #[test]
    fn falls_back_to_the_runtime_crate() {
        let found = [("navree-kernel", FoundCrate::Name("navree_kernel".into()))];
        assert_eq!(path(&found).as_deref(), Some(":: navree_kernel"));
    }

    #[test]
    fn inside_the_runtime_crate_uses_crate() {
        let found = [("navree-kernel", FoundCrate::Itself)];
        assert_eq!(path(&found).as_deref(), Some("crate"));
    }

    #[test]
    fn none_when_neither_is_a_dependency() {
        assert_eq!(path(&[]), None);
    }
}
