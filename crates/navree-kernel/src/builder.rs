//! [`ModuleBuilder`] and [`Factory`]: how a module describes what it provides.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use crate::lifecycle::LifecycleDef;
use crate::{
    BoxError, BoxFuture, CommandSpec, Injectable, IntoModules, KernelError, Lifecycle, Module,
    ModuleId, ProviderKey, Resolver, Scope,
};

/// A contributed value, typed by `TypeId`, with the type's name for the
/// per-type counts of the plugins report.
pub(crate) struct Contribution {
    /// The type of the value.
    pub(crate) type_id: TypeId,
    /// `std::any::type_name` of the value's type.
    pub(crate) type_name: &'static str,
    /// The value, an `Arc<C>` behind `dyn Any`.
    pub(crate) value: Arc<dyn Any + Send + Sync>,
}

/// A constructed provider value: an `Arc<T>` behind `dyn Any`.
pub(crate) type Instance = Arc<dyn Any + Send + Sync>;

/// A type-erased provider constructor.
pub(crate) type ErasedFactory =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Instance, KernelError>> + Send + Sync>;

/// One provider registration.
#[derive(Clone)]
pub(crate) struct ProviderDef {
    pub(crate) key: ProviderKey,
    pub(crate) scope: Scope,
    pub(crate) deps: Vec<ProviderKey>,
    pub(crate) factory: ErasedFactory,
    /// A multi-binding: collected by `get_all`, invisible to `get`.
    pub(crate) multi: bool,
}

/// Collects what a module registers. Passed to [`Module::register`].
pub struct ModuleBuilder {
    pub(crate) id: ModuleId,
    pub(crate) name: &'static str,
    pub(crate) global: bool,
    pub(crate) imports: Vec<Box<dyn Module>>,
    pub(crate) providers: Vec<ProviderDef>,
    pub(crate) exports: Vec<ProviderKey>,
    pub(crate) contributions: Vec<Contribution>,
    pub(crate) lifecycles: Vec<LifecycleDef>,
    pub(crate) commands: Vec<CommandSpec>,
}

impl ModuleBuilder {
    pub(crate) fn new(id: ModuleId, name: &'static str) -> Self {
        Self {
            id,
            name,
            global: false,
            imports: Vec::new(),
            providers: Vec::new(),
            exports: Vec::new(),
            contributions: Vec::new(),
            lifecycles: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// The id of the module being registered.
    pub fn module_id(&self) -> ModuleId {
        self.id
    }

    /// Imports a module: its exports become visible to this module.
    pub fn import(&mut self, module: impl Module) -> &mut Self {
        self.imports.push(Box::new(module));
        self
    }

    /// Imports several modules (a tuple of up to 16 modules or a `Vec<Box<dyn Module>>`).
    pub fn imports(&mut self, modules: impl IntoModules) -> &mut Self {
        self.imports.extend(modules.into_modules());
        self
    }

    /// Marks the module global: its exports are visible to every module.
    pub fn global(&mut self) -> &mut Self {
        self.global = true;
        self
    }

    /// Registers `T` under its unnamed key.
    pub fn provider<T: Injectable>(&mut self) -> &mut Self {
        self.injectable::<T>(ProviderKey::of::<T>())
    }

    /// Registers `T` under `name` (resolved with [`Resolver::get_named`]).
    pub fn provider_named<T: Injectable>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
    ) -> &mut Self {
        self.injectable::<T>(ProviderKey::named::<T>(name))
    }

    fn injectable<T: Injectable>(&mut self, key: ProviderKey) -> &mut Self {
        self.providers.push(ProviderDef {
            key,
            scope: T::scope(),
            deps: T::dependencies(),
            factory: Arc::new(|r| {
                Box::pin(async move {
                    T::construct(r)
                        .await
                        .map(|v| Arc::new(Arc::new(v)) as Instance)
                })
            }),
            multi: false,
        });
        self
    }

    /// Registers an already-built singleton value.
    pub fn value<T: Send + Sync + 'static>(&mut self, value: T) -> &mut Self {
        self.value_arc(Arc::new(value))
    }

    /// Registers an already-built singleton behind an `Arc` (also for `Arc<dyn Trait>`).
    pub fn value_arc<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) -> &mut Self {
        self.providers
            .push(value_def(ProviderKey::of::<T>(), value));
        self
    }

    /// Registers a provider built by a [`Factory`] (unnamed unless [`Factory::named`] was used).
    pub fn factory<T: ?Sized + Send + Sync + 'static>(&mut self, f: Factory<T>) -> &mut Self {
        self.providers.push(f.into_def());
        self
    }

    /// Adds a multi-binding of `T`. Every multi-binding of `T` in the app (in
    /// module dependency order) is returned by [`Resolver::get_all`] and
    /// [`AppContext::get_all`](crate::AppContext::get_all); `get::<T>()` does not see them.
    pub fn multi<T: ?Sized + Send + Sync + 'static>(&mut self, f: Factory<T>) -> &mut Self {
        let mut def = f.into_def();
        def.multi = true;
        self.providers.push(def);
        self
    }

    /// Exports the unnamed provider of `T` to importing modules.
    pub fn exports<T: ?Sized + 'static>(&mut self) -> &mut Self {
        self.export_key(ProviderKey::of::<T>())
    }

    /// Exports a provider key (named or unnamed) to importing modules.
    pub fn export_key(&mut self, key: ProviderKey) -> &mut Self {
        self.exports.push(key);
        self
    }

    /// Contributes a value that other modules collect with
    /// [`AppContext::contributions`](crate::AppContext::contributions)
    /// (routes, migrations, job registrations, ...).
    pub fn contribute<C: Send + Sync + 'static>(&mut self, c: C) -> &mut Self {
        self.contributions.push(Contribution {
            type_id: TypeId::of::<C>(),
            type_name: std::any::type_name::<C>(),
            value: Arc::new(c),
        });
        self
    }

    /// Runs the [`Lifecycle`] hooks of provider `T`, which must be a provider
    /// of this module.
    pub fn lifecycle<T: Lifecycle + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.lifecycles.push(LifecycleDef::of::<T>());
        self
    }

    /// Registers a CLI command (names must be unique across the app).
    pub fn command(&mut self, cmd: CommandSpec) -> &mut Self {
        self.commands.push(cmd);
        self
    }
}

impl fmt::Debug for ModuleBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModuleBuilder")
            .field("module", &self.name)
            .field("global", &self.global)
            .field("imports", &self.imports.len())
            .field(
                "providers",
                &self
                    .providers
                    .iter()
                    .map(|p| p.key.to_string())
                    .collect::<Vec<_>>(),
            )
            .field(
                "exports",
                &self
                    .exports
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// A provider definition for an `Arc<T>` built by an async closure. Use it for
/// trait objects (`Factory::<dyn Mailer>::new(...)`), named providers and
/// values that need async setup.
pub struct Factory<T: ?Sized> {
    scope: Scope,
    deps: Vec<ProviderKey>,
    name: Option<Cow<'static, str>>,
    build: BuildFn<T>,
}

/// The closure behind a [`Factory`].
type BuildFn<T> =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>, BoxError>> + Send + Sync>;

impl<T: ?Sized + Send + Sync + 'static> Factory<T> {
    /// A singleton factory with no declared dependencies.
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn(Resolver) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<T>, BoxError>> + Send + 'static,
    {
        Self {
            scope: Scope::Singleton,
            deps: Vec::new(),
            name: None,
            build: Arc::new(move |r| Box::pin(f(r))),
        }
    }

    /// Sets the provider scope.
    pub fn scope(mut self, s: Scope) -> Self {
        self.scope = s;
        self
    }

    /// Declares the keys the closure resolves, for the boot-time graph check.
    pub fn deps(mut self, d: Vec<ProviderKey>) -> Self {
        self.deps = d;
        self
    }

    /// Registers the provider under a name instead of the unnamed key.
    pub fn named(mut self, n: impl Into<Cow<'static, str>>) -> Self {
        self.name = Some(n.into());
        self
    }

    pub(crate) fn into_def(self) -> ProviderDef {
        let key = match self.name {
            Some(name) => ProviderKey::named::<T>(name),
            None => ProviderKey::of::<T>(),
        };
        let provider = key.to_string();
        let build = self.build;
        ProviderDef {
            key,
            scope: self.scope,
            deps: self.deps,
            factory: Arc::new(move |r| {
                let fut = build(r);
                let provider = provider.clone();
                Box::pin(async move {
                    match fut.await {
                        Ok(value) => Ok(Arc::new(value) as Instance),
                        Err(error) => Err(into_kernel_error(error, provider)),
                    }
                })
            }),
            multi: false,
        }
    }
}

impl<T: ?Sized> fmt::Debug for Factory<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Factory")
            .field("type", &std::any::type_name::<T>())
            .field("scope", &self.scope)
            .field("deps", &self.deps)
            .field("name", &self.name)
            .finish()
    }
}

/// A singleton definition that always yields `value`.
pub(crate) fn value_def<T: ?Sized + Send + Sync + 'static>(
    key: ProviderKey,
    value: Arc<T>,
) -> ProviderDef {
    ProviderDef {
        key,
        scope: Scope::Singleton,
        deps: Vec::new(),
        factory: Arc::new(move |_| {
            let instance: Instance = Arc::new(value.clone());
            Box::pin(async move { Ok(instance) })
        }),
        multi: false,
    }
}

/// Keeps resolution errors (missing provider, cycle, ...) raised inside a
/// factory as they are; wraps any other error as a construction failure.
fn into_kernel_error(error: BoxError, provider: String) -> KernelError {
    match error.downcast::<KernelError>() {
        Ok(kernel) => *kernel,
        Err(source) => KernelError::Construction { provider, source },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribute_records_the_type_name_for_the_counts() {
        struct Notice;
        let mut m = ModuleBuilder::new(ModuleId::of::<Notice>(), "Notice");
        m.contribute(1_u8)
            .contribute(2_u8)
            .contribute(String::from("Ludhiana"));
        let names: Vec<&str> = m.contributions.iter().map(|c| c.type_name).collect();
        assert_eq!(names, ["u8", "u8", "alloc::string::String"]);
        assert_eq!(m.contributions[2].type_id, TypeId::of::<String>());
    }
}
