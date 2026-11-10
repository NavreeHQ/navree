//! Kernel errors. Every wiring error names the module and the type involved.

use std::fmt;

use crate::BoxError;

/// An error raised while validating the module graph, resolving a provider,
/// running a lifecycle hook or dispatching a command.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KernelError {
    /// No provider for `requested` is visible from `module`.
    #[error(
        "module `{module}` cannot resolve `{requested}` (required by {required_by}): no provider is registered in `{module}`, exported by a module it imports, or exported by a global module"
    )]
    MissingProvider {
        /// The module doing the resolving.
        module: &'static str,
        /// The requested provider key.
        requested: String,
        /// What needed it (a provider, `exports`, a lifecycle registration, ...).
        required_by: String,
    },
    /// The provider exists in an imported module but that module does not export it.
    #[error(
        "module `{module}` cannot use `{requested}`: it is private to module `{owner}`; add it to the exports of `{owner}`"
    )]
    NotExported {
        /// The module doing the resolving.
        module: &'static str,
        /// The requested provider key.
        requested: String,
        /// The module that registers the provider.
        owner: &'static str,
    },
    /// The provider is exported by a module that the requesting module does not import.
    #[error(
        "module `{module}` cannot use `{requested}`: it is exported by module `{owner}`, which `{module}` does not import"
    )]
    NotImported {
        /// The module doing the resolving.
        module: &'static str,
        /// The requested provider key.
        requested: String,
        /// The module that registers and exports the provider.
        owner: &'static str,
    },
    /// Several different providers of the key are visible (e.g. two imports export it).
    #[error(
        "module `{module}` sees `{requested}` from several modules ({}); export it from one module only, or use named providers",
        owners.join(", ")
    )]
    AmbiguousProvider {
        /// The module doing the resolving.
        module: &'static str,
        /// The requested provider key.
        requested: String,
        /// The modules that own the competing providers.
        owners: Vec<&'static str>,
    },
    /// A provider dependency cycle (`A -> B -> A`).
    #[error("provider dependency cycle: {}", path.join(" -> "))]
    ProviderCycle {
        /// The providers on the cycle; the first and last entries are the same.
        path: Vec<String>,
    },
    /// A module import cycle (`AModule -> BModule -> AModule`).
    #[error("module import cycle: {}", path.join(" -> "))]
    ModuleCycle {
        /// The modules on the cycle; the first and last entries are the same.
        path: Vec<&'static str>,
    },
    /// A singleton depends (directly or through transients) on a request-scoped provider.
    #[error(
        "scope inversion in module `{module}`: singleton `{singleton}` depends on request-scoped `{request_scoped}`; take request values as parameters or extractors instead"
    )]
    ScopeInversion {
        /// The module that registers the singleton.
        module: &'static str,
        /// The singleton provider.
        singleton: String,
        /// The request-scoped provider it reaches.
        request_scoped: String,
    },
    /// A request-scoped provider was resolved without a [`RequestScope`](crate::RequestScope).
    #[error(
        "request-scoped provider `{provider}` was resolved outside a request scope; resolve it through `AppContext::request_resolver`"
    )]
    MissingRequestScope {
        /// The request-scoped provider.
        provider: String,
    },
    /// Two configured (`for_root`) instances of the same module.
    #[error(
        "module `{module}` is configured more than once; call `for_root` once (usually in the app root) and import the module by type elsewhere"
    )]
    ConfiguredTwice {
        /// The module configured twice.
        module: &'static str,
    },
    /// The same provider key registered twice in one module.
    #[error("module `{module}` registers provider `{key}` more than once")]
    DuplicateProvider {
        /// The module.
        module: &'static str,
        /// The duplicated key.
        key: String,
    },
    /// A provider factory or constructor failed.
    #[error("failed to construct `{provider}`: {source}")]
    Construction {
        /// The provider being constructed.
        provider: String,
        /// The underlying error.
        source: BoxError,
    },
    /// No command with this name is registered.
    #[error("unknown command `{name}`; run `help` to list the available commands")]
    UnknownCommand {
        /// The requested command name.
        name: String,
    },
    /// Two modules register a command with the same name.
    #[error("command `{name}` is registered by both `{first}` and `{second}`")]
    DuplicateCommand {
        /// The command name.
        name: String,
        /// The module that registered it first.
        first: &'static str,
        /// The module that registered it again.
        second: &'static str,
    },
    /// A lifecycle hook failed.
    #[error("`{hook}` hook of module `{module}` failed: {source}")]
    Lifecycle {
        /// The module whose provider implements the hook.
        module: &'static str,
        /// `on_init`, `on_bootstrap` or `on_shutdown`.
        hook: &'static str,
        /// The underlying error.
        source: BoxError,
    },
}

/// Every error found while booting or validating an app.
#[derive(Debug)]
pub struct BootErrors(pub Vec<KernelError>);

impl fmt::Display for BootErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, error) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for BootErrors {}

impl From<KernelError> for BootErrors {
    fn from(error: KernelError) -> Self {
        Self(vec![error])
    }
}
