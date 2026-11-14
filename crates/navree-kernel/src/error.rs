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
    /// A module calls `m.plugin(..)` more than once.
    #[error("module `{module}` calls m.plugin(..) twice; call it once in its register")]
    PluginTwice {
        /// The module.
        module: &'static str,
    },
    /// A plugin's namespace is not a lower-case letter followed by up to 63
    /// lower-case letters, digits or `_`.
    #[error(
        "plugin `{plugin}` in module `{module}` has the namespace `{namespace}`, which is not a lower-case letter followed by up to 63 lower-case letters, digits or _; set one with plugin_info!(namespace = \"..\")"
    )]
    InvalidNamespace {
        /// The module that names the plugin.
        module: &'static str,
        /// The plugin's name.
        plugin: &'static str,
        /// The namespace it gives.
        namespace: &'static str,
    },
    /// A module names a plugin of another crate: its type names no path of the
    /// plugin's crate, or another module gives the plugin another crate.
    #[error(
        "module `{module}` names the plugin `{plugin}`, which is another crate's; a plugin's modules are in its own crate (a ClientModule names it through an options type of the plugin's crate)"
    )]
    PluginOfAnotherCrate {
        /// The module.
        module: &'static str,
        /// The plugin's name.
        plugin: &'static str,
    },
    /// A plugin that is not first-party uses a namespace the framework keeps.
    #[error(
        "plugin `{plugin}` uses the namespace `{namespace}`, which the framework keeps for itself; set another with plugin_info!(namespace = \"..\")"
    )]
    NamespaceReserved {
        /// The plugin's name.
        plugin: &'static str,
        /// The reserved namespace.
        namespace: &'static str,
    },
    /// Two modules name one plugin at two versions.
    #[error(
        "plugin `{plugin}` is in the app at two versions ({first} in `{first_module}`, {second} in `{second_module}`); depend on one version"
    )]
    PluginVersions {
        /// The plugin's name.
        plugin: &'static str,
        /// The version the first module gives.
        first: &'static str,
        /// The first module.
        first_module: &'static str,
        /// The other version.
        second: &'static str,
        /// The module that gives it.
        second_module: &'static str,
    },
    /// Two modules describe one plugin (one name, crate and version) differently.
    #[error(
        "plugin `{plugin}` is described differently by `{first_module}` and `{second_module}`; give every module of a plugin the same plugin_info!(..)"
    )]
    PluginInfoDiffers {
        /// The plugin's name.
        plugin: &'static str,
        /// The module whose description counts.
        first_module: &'static str,
        /// A module that describes the plugin otherwise.
        second_module: &'static str,
    },
    /// Two plugins use one namespace.
    #[error(
        "plugins `{first}` and `{second}` both use the namespace `{namespace}`; set another with plugin_info!(namespace = \"..\") in one of them"
    )]
    NamespaceTaken {
        /// The namespace.
        namespace: &'static str,
        /// The plugin that uses it first.
        first: &'static str,
        /// The plugin that uses it too.
        second: &'static str,
    },
    /// A module that does not name itself a plugin has files to publish.
    #[error(
        "module `{module}` has files to publish but does not name itself a plugin; call m.plugin(plugin_info!()) in its register"
    )]
    PublishableOutsidePlugin {
        /// The module.
        module: &'static str,
    },
    /// A file a plugin ships has a path that is not safe inside its folder.
    #[error(
        "plugin `{plugin}`: the {tag} file `{path}` is not a safe relative path inside its folder (no `..`, no leading `/`, no `\\`, no `:`, none of `<>\"|?*`, no segment ending in `.` or a space, no device name such as `con` or `nul`, no two names that differ only in case, no file that is also a folder); fix the path in the plugin"
    )]
    UnsafePublishPath {
        /// The plugin's name.
        plugin: &'static str,
        /// The file's tag: `views`, `locales` or `files`.
        tag: &'static str,
        /// The file's path, as the plugin gives it.
        path: String,
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
