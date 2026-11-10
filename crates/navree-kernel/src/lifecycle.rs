//! The [`Lifecycle`] trait: hooks run at boot and at shutdown.

use std::sync::Arc;

use crate::{AppContext, BoxError, BoxFuture, KernelError, ProviderKey, Resolver};

/// Hooks implemented by a provider and registered with
/// [`ModuleBuilder::lifecycle`](crate::ModuleBuilder::lifecycle).
///
/// At boot, after every singleton is built, `on_init` runs for every
/// registered provider in module dependency order, then `on_bootstrap` in the
/// same order. At shutdown, `on_shutdown` runs in reverse order.
pub trait Lifecycle: Send + Sync + 'static {
    /// Runs once all singletons exist.
    fn on_init(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    /// Runs after every `on_init` succeeded.
    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }

    /// Runs during graceful shutdown, after tracked tasks have drained.
    fn on_shutdown(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async { Ok(()) })
    }
}

/// Resolves a registered lifecycle provider as `Arc<dyn Lifecycle>`.
pub(crate) type LifecycleResolve = Arc<
    dyn Fn(Resolver) -> BoxFuture<'static, Result<Arc<dyn Lifecycle>, KernelError>> + Send + Sync,
>;

/// A `lifecycle::<T>()` registration.
#[derive(Clone)]
pub(crate) struct LifecycleDef {
    pub(crate) key: ProviderKey,
    pub(crate) resolve: LifecycleResolve,
}

impl LifecycleDef {
    pub(crate) fn of<T: Lifecycle>() -> Self {
        Self {
            key: ProviderKey::of::<T>(),
            resolve: Arc::new(|r| {
                Box::pin(async move { r.get::<T>().await.map(|hooks| hooks as Arc<dyn Lifecycle>) })
            }),
        }
    }
}
