//! Per-operation dataloaders: the [`Loader`] trait an app's loaders
//! implement, and [`RequestLoader`], which batches and caches one
//! operation's loads.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use navree_kernel::{BoxFuture, Injectable, KernelError, ModuleId, Resolver};
use tokio::sync::oneshot;

/// How long a dataloader collects keys before loading them, so the loads
/// that an operation's fields ask for together go out as one batch.
const BATCH_DELAY: Duration = Duration::from_millis(1);

/// The most keys of one batch: a batch this full is loaded at once.
const MAX_BATCH_SIZE: usize = 1000;

/// Loads values by key, many keys at a time: what a loader registered with
/// `m.dataloader::<L>()` implements, once per key type.
///
/// ```
/// use std::collections::HashMap;
/// use std::sync::Arc;
///
/// use navree_graphql::Loader;
///
/// /// Names by id, one query per batch.
/// struct NameLoader;
///
/// impl Loader<u64> for NameLoader {
///     type Value = String;
///     type Error = Arc<std::io::Error>;
///
///     async fn load(&self, ids: &[u64]) -> Result<HashMap<u64, String>, Self::Error> {
///         Ok(ids.iter().map(|id| (*id, format!("name {id}"))).collect())
///     }
/// }
/// ```
pub trait Loader<K>: Send + Sync + 'static
where
    K: Send + Sync + Hash + Eq + Clone + 'static,
{
    /// The value of one key.
    type Value: Send + Sync + Clone + 'static;

    /// The error of a failed batch, given to every load waiting on it.
    type Error: Send + Clone + 'static;

    /// The values of `keys` (each key appears once). A key missing from the
    /// map has no value: `load_one` gives `None` for it.
    fn load(
        &self,
        keys: &[K],
    ) -> impl Future<Output = Result<HashMap<K, Self::Value>, Self::Error>> + Send;
}

/// The dataloader of one GraphQL operation for loader `L`
/// (`ctx.loader::<L>()`): it batches the loads requested in the same tick
/// into one call to [`Loader::load`] and caches the results for the rest of
/// the operation only, so values are never shared between two viewers.
/// Errors and missing keys are not cached.
pub struct RequestLoader<L> {
    loader: Arc<L>,
    /// One [`Keyed`] per key type, by the key type's id.
    state: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send>>>>,
}

impl<L> fmt::Debug for RequestLoader<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RequestLoader")
            .field(&std::any::type_name::<L>())
            .finish()
    }
}

impl<L: Send + Sync + 'static> RequestLoader<L> {
    /// A dataloader over the app's instance of `L`, with an empty cache.
    pub fn new(loader: Arc<L>) -> Self {
        Self {
            loader,
            state: Arc::default(),
        }
    }

    /// The app's instance of `L`.
    pub fn loader(&self) -> &Arc<L> {
        &self.loader
    }

    /// The value of `key`, or `None` when the loader has none.
    pub async fn load_one<K>(&self, key: K) -> Result<Option<L::Value>, L::Error>
    where
        K: Send + Sync + Hash + Eq + Clone + 'static,
        L: Loader<K>,
    {
        let mut values = self.load_many(std::iter::once(key.clone())).await?;
        Ok(values.remove(&key))
    }

    /// The values of `keys` that the loader has: cached ones at once, the
    /// others with the next batch.
    pub async fn load_many<K, I>(&self, keys: I) -> Result<HashMap<K, L::Value>, L::Error>
    where
        K: Send + Sync + Hash + Eq + Clone + 'static,
        I: IntoIterator<Item = K>,
        L: Loader<K>,
    {
        let (reply, start) = {
            let mut state = lock(&self.state);
            let keyed = keyed::<K, L::Value, L::Error>(&mut state);
            let mut found = HashMap::new();
            let mut missing = HashSet::new();
            for key in keys {
                match keyed.cache.get(&key) {
                    Some(value) => {
                        found.insert(key, value.clone());
                    }
                    None => {
                        missing.insert(key);
                    }
                }
            }
            if missing.is_empty() {
                return Ok(found);
            }
            let first = keyed.pending.is_empty();
            keyed.pending.extend(missing.iter().cloned());
            let (sender, reply) = oneshot::channel();
            keyed.waiting.push(Waiter {
                keys: missing,
                found,
                reply: sender,
            });
            let start = if keyed.pending.len() >= MAX_BATCH_SIZE {
                Some(Start::Now(keyed.take()))
            } else if first {
                Some(Start::AfterDelay)
            } else {
                None
            };
            (reply, start)
        };
        match start {
            Some(Start::Now(batch)) => {
                tokio::spawn(run::<K, L>(self.loader.clone(), self.state.clone(), batch));
            }
            Some(Start::AfterDelay) => {
                let loader = self.loader.clone();
                let state = self.state.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(BATCH_DELAY).await;
                    let batch = keyed::<K, L::Value, L::Error>(&mut lock(&state)).take();
                    if !batch.keys.is_empty() {
                        run::<K, L>(loader, state, batch).await;
                    }
                });
            }
            None => {}
        }
        reply
            .await
            .expect("the dataloader's batch did not finish: its loader panicked")
    }
}

/// When a load starts its batch.
enum Start<K, V, E> {
    /// Now: the batch is full.
    Now(Batch<K, V, E>),
    /// After [`BATCH_DELAY`]: this load is the batch's first.
    AfterDelay,
}

/// The cache and the batch being collected, for one key type.
struct Keyed<K, V, E> {
    cache: HashMap<K, V>,
    pending: HashSet<K>,
    waiting: Vec<Waiter<K, V, E>>,
}

impl<K, V, E> Default for Keyed<K, V, E> {
    fn default() -> Self {
        Self {
            cache: HashMap::new(),
            pending: HashSet::new(),
            waiting: Vec::new(),
        }
    }
}

impl<K, V, E> Keyed<K, V, E> {
    /// The batch collected so far, leaving an empty one.
    fn take(&mut self) -> Batch<K, V, E> {
        Batch {
            keys: std::mem::take(&mut self.pending),
            waiting: std::mem::take(&mut self.waiting),
        }
    }
}

/// The keys of one call to [`Loader::load`] and the loads waiting on it.
struct Batch<K, V, E> {
    keys: HashSet<K>,
    waiting: Vec<Waiter<K, V, E>>,
}

/// One `load_many` waiting on a batch.
struct Waiter<K, V, E> {
    /// The keys it waits for.
    keys: HashSet<K>,
    /// The values it found in the cache.
    found: HashMap<K, V>,
    reply: oneshot::Sender<Result<HashMap<K, V>, E>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The state of key type `K`, created on first use.
fn keyed<K, V, E>(state: &mut HashMap<TypeId, Box<dyn Any + Send>>) -> &mut Keyed<K, V, E>
where
    K: Send + 'static,
    V: Send + 'static,
    E: Send + 'static,
{
    state
        .entry(TypeId::of::<K>())
        .or_insert_with(|| Box::new(Keyed::<K, V, E>::default()))
        .downcast_mut::<Keyed<K, V, E>>()
        .expect("a loader has one value and error type per key type")
}

/// Loads `batch`, caches what came back and answers every waiting load.
async fn run<K, L>(
    loader: Arc<L>,
    state: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send>>>>,
    batch: Batch<K, L::Value, L::Error>,
) where
    K: Send + Sync + Hash + Eq + Clone + 'static,
    L: Loader<K>,
{
    let keys: Vec<K> = batch.keys.into_iter().collect();
    match loader.load(&keys).await {
        Ok(values) => {
            keyed::<K, L::Value, L::Error>(&mut lock(&state))
                .cache
                .extend(
                    values
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            for waiter in batch.waiting {
                let mut found = waiter.found;
                for key in waiter.keys {
                    if let Some(value) = values.get(&key) {
                        found.insert(key, value.clone());
                    }
                }
                // The field may have stopped waiting.
                let _ = waiter.reply.send(Ok(found));
            }
        }
        Err(error) => {
            for waiter in batch.waiting {
                let _ = waiter.reply.send(Err(error.clone()));
            }
        }
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
                        Box::new(RequestLoader::<L>::new(loader.clone()))
                            as Box<dyn Any + Send + Sync>
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Doubles numbers; odd batches of one key fail; records every batch.
    #[derive(Default)]
    struct Doubler {
        batches: Mutex<Vec<Vec<u32>>>,
        calls: AtomicUsize,
    }

    impl Loader<u32> for Doubler {
        type Value = u32;
        type Error = String;

        async fn load(&self, keys: &[u32]) -> Result<HashMap<u32, u32>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut sorted = keys.to_vec();
            sorted.sort_unstable();
            lock(&self.batches).push(sorted);
            if keys == [13] {
                return Err("thirteen".into());
            }
            // No value for zero.
            Ok(keys
                .iter()
                .filter(|key| **key != 0)
                .map(|key| (*key, key * 2))
                .collect())
        }
    }

    impl Loader<&'static str> for Doubler {
        type Value = usize;
        type Error = String;

        async fn load(
            &self,
            keys: &[&'static str],
        ) -> Result<HashMap<&'static str, usize>, String> {
            Ok(keys.iter().map(|key| (*key, key.len())).collect())
        }
    }

    #[tokio::test]
    async fn loads_of_one_tick_are_one_batch_without_duplicates() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        let (a, b, c, many) = tokio::join!(
            loader.load_one(1u32),
            loader.load_one(2u32),
            loader.load_one(1u32),
            loader.load_many([3u32, 0]),
        );
        assert_eq!(
            (a.unwrap(), b.unwrap(), c.unwrap()),
            (Some(2), Some(4), Some(2))
        );
        assert_eq!(many.unwrap(), HashMap::from([(3, 6)]), "zero has no value");
        assert_eq!(*lock(&loader.loader().batches), vec![vec![0, 1, 2, 3]]);
    }

    #[tokio::test]
    async fn results_are_cached_for_the_operation_but_misses_are_not() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        assert_eq!(loader.load_one(5u32).await.unwrap(), Some(10));
        assert_eq!(loader.load_one(0u32).await.unwrap(), None);
        // 5 comes from the cache; 0 and 6 are loaded.
        let values = loader.load_many([5u32, 6, 0]).await.unwrap();
        assert_eq!(values, HashMap::from([(5, 10), (6, 12)]));
        assert_eq!(
            *lock(&loader.loader().batches),
            vec![vec![5], vec![0], vec![0, 6]]
        );
        // All cached: no call at all.
        let calls = loader.loader().calls.load(Ordering::SeqCst);
        assert_eq!(loader.load_many([5u32, 6]).await.unwrap().len(), 2);
        assert_eq!(loader.loader().calls.load(Ordering::SeqCst), calls);
        // A new operation's dataloader starts empty.
        let next = RequestLoader::new(loader.loader().clone());
        assert_eq!(next.load_one(5u32).await.unwrap(), Some(10));
        assert_eq!(loader.loader().calls.load(Ordering::SeqCst), calls + 1);
    }

    #[tokio::test]
    async fn a_failed_batch_fails_every_load_waiting_on_it() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        let (a, b) = tokio::join!(loader.load_one(13u32), loader.load_many([13u32]));
        assert_eq!(a.unwrap_err(), "thirteen");
        assert_eq!(b.unwrap_err(), "thirteen");
        // Errors are not cached: the next load tries again.
        assert!(loader.load_one(13u32).await.is_err());
        assert_eq!(lock(&loader.loader().batches).len(), 2);
    }

    #[tokio::test]
    async fn each_key_type_has_its_own_batches_and_cache() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        let (number, text) = tokio::join!(loader.load_one(4u32), loader.load_one("four"));
        assert_eq!((number.unwrap(), text.unwrap()), (Some(8), Some(4)));
        assert_eq!(*lock(&loader.loader().batches), vec![vec![4]]);
    }

    #[tokio::test]
    async fn a_full_batch_loads_at_once() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        let keys: Vec<u32> = (1..=MAX_BATCH_SIZE as u32 + 5).collect();
        let values = loader.load_many(keys.clone()).await.unwrap();
        assert_eq!(values.len(), keys.len());
        assert_eq!(lock(&loader.loader().batches).len(), 1);
    }

    #[tokio::test]
    async fn no_keys_load_nothing() {
        let loader = RequestLoader::new(Arc::new(Doubler::default()));
        assert!(
            loader
                .load_many(std::iter::empty::<u32>())
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(loader.loader().calls.load(Ordering::SeqCst), 0);
    }
}
