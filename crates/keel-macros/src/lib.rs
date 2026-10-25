//! # keel-macros
//!
//! Attribute macros for Keel applications: `#[module]`, `#[injectable]`,
//! `#[controller]` and the route attributes.
//!
//! Every attribute expands into plain Rust that calls the runtime crates; the
//! hand-written `impl Module` / `impl Injectable` is always available as the
//! escape hatch (framework crates use it and never these macros).
//!
//! ## Paths
//!
//! The generated code names the runtime crates through the `keel` facade when
//! the crate being compiled depends on it (`::keel::kernel::…`,
//! `::keel::http::…`, honouring a rename in `Cargo.toml`), and otherwise
//! through the individual crates (`::keel_kernel::…`, `::keel_http::…`).
//! Depending on neither is a compile error that names the missing crate.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod attr;
mod injectable;
mod module;
mod paths;

/// Implements `Injectable` for a struct by resolving each field from the
/// container (constructor injection).
///
/// ```text
/// #[injectable]                                  // singleton (the default)
/// #[injectable(scope = "singleton" | "request" | "transient")]
/// ```
///
/// Each named field is filled according to its type:
///
/// | Field | Filled with | Dependency edge |
/// |---|---|---|
/// | `Arc<T>` (also `Arc<dyn Trait>`) | `resolver.get::<T>()` | `ProviderKey::of::<T>()` |
/// | `#[inject(name = "x")] Arc<T>` | `resolver.get_named::<T>("x")` | `ProviderKey::named::<T>("x")` |
/// | `Lazy<T>` | `resolver.lazy::<T>()`, resolved on first `get` after boot | none (it breaks cycles) |
/// | `#[inject(default)] T` | `Default::default()` | none |
///
/// `dependencies()` lists the edges in field order, so the boot-time graph
/// check sees them. Unit structs (and `struct S {}`) are allowed; generic
/// structs keep their parameters and bounds. Any other field type, a tuple
/// struct, an enum, an unknown argument or an unknown scope is a compile error
/// that points at the offending tokens.
///
/// ```
/// use std::sync::Arc;
///
/// use keel_kernel::{Injectable, Keel, Lazy, Module, ModuleBuilder, Scope};
/// use keel_macros::injectable;
///
/// #[injectable]
/// struct Repo;
///
/// #[injectable]
/// struct Mailer;
///
/// #[injectable(scope = "transient")]
/// struct UserService {
///     repo: Arc<Repo>,
///     #[inject(name = "audit")]
///     audit_repo: Arc<Repo>,
///     mailer: Lazy<Mailer>,
///     #[inject(default)]
///     greeting: String,
/// }
///
/// assert_eq!(UserService::scope(), Scope::Transient);
/// assert_eq!(UserService::dependencies().len(), 2); // `mailer` and `greeting` are not edges
/// ```
#[proc_macro_attribute]
pub fn injectable(args: TokenStream, item: TokenStream) -> TokenStream {
    injectable::expand(args.into(), item.into()).into()
}

/// Implements `Module` for a struct from a declarative description.
///
/// ```text
/// #[module(
///     imports = [DatabaseModule, CacheModule::for_feature(opts)],  // expressions
///     providers = [UserRepository, UserService],                    // #[injectable] types
///     controllers = [UserController],                               // needs keel-http
///     exports = [UserService],                                      // types (dyn Trait too)
///     global,                                                       // exports visible everywhere
///     register = Self::extra,                                       // fn(&Self, &mut ModuleBuilder)
/// )]
/// ```
///
/// Every key is optional; order inside the attribute does not matter. The
/// generated `register` calls, in this order: `m.import(expr)` per import,
/// `m.provider::<P>()` per provider,
/// `keel_http::ModuleBuilderHttpExt::controller::<C>(m)` per controller,
/// `m.exports::<E>()` per export, `m.global()`, and finally the `register`
/// function or closure, for everything the attribute does not declare
/// (configuration sections, values, factories, named providers, migrations,
/// jobs, commands, lifecycle hooks). Unknown or duplicate keys are compile
/// errors pointing at the key.
///
/// ```
/// use std::sync::Arc;
///
/// use keel_kernel::{Keel, ModuleBuilder};
/// use keel_macros::{injectable, module};
///
/// #[injectable]
/// struct Repo;
///
/// #[module(providers = [Repo], exports = [Repo])]
/// struct DataModule;
///
/// struct Greeting(&'static str);
///
/// #[injectable]
/// struct UserService {
///     repo: Arc<Repo>,
///     greeting: Arc<Greeting>,
/// }
///
/// #[module(
///     imports = [DataModule],
///     providers = [UserService],
///     exports = [UserService],
///     register = Self::extra,
/// )]
/// struct UsersModule {
///     greeting: &'static str,
/// }
///
/// impl UsersModule {
///     fn extra(&self, m: &mut ModuleBuilder) {
///         m.value(Greeting(self.greeting));
///     }
/// }
///
/// let report = Keel::app()
///     .module(UsersModule { greeting: "hello" })
///     .validate()
///     .expect("the module graph is valid");
/// let users = report.modules.iter().find(|m| m.name == "UsersModule").unwrap();
/// assert_eq!(users.imports, ["DataModule"]);
/// assert_eq!(users.providers.len(), 2); // UserService and the Greeting value
/// ```
#[proc_macro_attribute]
pub fn module(args: TokenStream, item: TokenStream) -> TokenStream {
    module::expand(args.into(), item.into()).into()
}
