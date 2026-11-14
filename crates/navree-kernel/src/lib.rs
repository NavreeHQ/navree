//! # navree-kernel
//!
//! The Navree kernel: module graph, type-keyed dependency injection container,
//! lifecycle hooks and the command runner. Everything else in Navree is a
//! [`Module`].
//!
//! ```no_run
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//!
//! struct GreetingModule;
//!
//! impl Module for GreetingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.value(String::from("hello")).exports::<String>();
//!     }
//! }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     Navree::app().module(GreetingModule).run()
//! }
//! ```
//!
//! Rules: a module resolves its own providers, the exports of modules it
//! imports directly and the exports of global modules. Singletons are built
//! once (eagerly at boot, in module dependency order) and shared as `Arc<T>`.
//! Wiring errors (missing or private providers, cycles, scope inversions,
//! modules configured twice) fail the boot and name the module and the type.
//! The kernel has no global statics.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(panic = "unwind"))]
compile_error!(
    "navree requires `panic = \"unwind\"`: a panicking handler must not abort the whole server"
);

mod app;
mod builder;
mod command;
mod container;
mod error;
mod graph;
mod introspect;
mod key;
mod lazy;
mod lifecycle;
mod module;
mod plugin;
mod plugin_graph;
mod resolver;

pub use app::{App, AppBuilder, AppContext, Contributed, ModuleInfo, Navree, WeakAppContext};
pub use builder::{Factory, ModuleBuilder};
pub use command::{CommandFn, CommandSpec};
pub use container::RequestScope;
pub use error::{BootErrors, KernelError};
pub use graph::{CommandInfo, GraphReport, ModuleReport, ProviderReport};
pub use key::{ModuleId, ProviderKey, Scope};
pub use lazy::Lazy;
pub use lifecycle::Lifecycle;
pub use module::{CodeConfigured, ConfiguredModule, Injectable, IntoModules, Module};
pub use plugin::PluginInfo;
pub use resolver::Resolver;

/// Items Navree's own crates and macros use; not public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::plugin::{
        FIRST_PARTY, RESERVED_NAMESPACES, default_namespace, is_namespace, namespace_text,
    };
}

/// A boxed error that can cross module and task boundaries.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A boxed, `Send` future (used where traits need to return futures as objects).
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
