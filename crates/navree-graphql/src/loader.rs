//! Per-operation dataloaders: [`RequestLoader`] around a shared
//! [`SharedLoader`].

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;

use async_graphql::dataloader::{DataLoader, HashMapCache, Loader};
use navree_kernel::{BoxFuture, Injectable, KernelError, ModuleId, Resolver};

/// The dataloader of one GraphQL operation for loader `L`
/// (`ctx.loader::<L>()`): it batches the loads requested in the same tick
/// into one call to `L::load` and caches the results for the rest of the
/// operation only, so values are never shared between two viewers.
pub type RequestLoader<L> = DataLoader<SharedLoader<L>, HashMapCache>;

/// The app's single instance of loader `L`, as each operation's
/// [`RequestLoader`] calls it. Implements [`Loader<K>`] whenever `L` does.
pub struct SharedLoader<L>(Arc<L>);

impl<L> SharedLoader<L> {
    /// Wraps the app's instance.
    pub fn new(loader: Arc<L>) -> Self {
        Self(loader)
    }

    /// The app's instance.
    pub fn inner(&self) -> &Arc<L> {
        &self.0
    }
}

impl<L> fmt::Debug for SharedLoader<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SharedLoader")
            .field(&std::any::type_name::<L>())
            .finish()
    }
}

impl<K, L> Loader<K> for SharedLoader<L>
where
    K: Send + Sync + Hash + Eq + Clone + 'static,
    L: Loader<K>,
{
    type Value = L::Value;
    type Error = L::Error;

    fn load(
        &self,
        keys: &[K],
    ) -> impl Future<Output = Result<HashMap<K, Self::Value>, Self::Error>> + Send {
        self.0.load(keys)
    }
}

/// Makes one operation's dataloader of a registered loader.
pub(crate) type MakeLoader = Arc<dyn Fn() -> Box<dyn Any + Send + Sync> + Send + Sync>;

/// A loader registered by a module (a contribution).
#[derive(Clone)]
pub(crate) struct LoaderRegistration {
    pub(crate) module: ModuleId,
    pub(crate) resolve:
        Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<LoaderEntry, KernelError>> + Send + Sync>,
}

/// A resolved loader: its type and how to make an operation's dataloader.
#[derive(Clone)]
pub(crate) struct LoaderEntry {
    pub(crate) type_id: TypeId,
    pub(crate) make: MakeLoader,
}

impl LoaderRegistration {
    pub(crate) fn of<L: Injectable + Send + Sync + 'static>(module: ModuleId) -> Self {
        Self {
            module,
            resolve: Arc::new(|r: Resolver| {
                Box::pin(async move {
                    let loader = r.get::<L>().await?;
                    let make: MakeLoader = Arc::new(move || {
                        Box::new(RequestLoader::<L>::with_cache(
                            SharedLoader(loader.clone()),
                            tokio::spawn,
                            HashMapCache::default(),
                        )) as Box<dyn Any + Send + Sync>
                    });
                    Ok(LoaderEntry {
                        type_id: TypeId::of::<L>(),
                        make,
                    })
                })
            }),
        }
    }
}
