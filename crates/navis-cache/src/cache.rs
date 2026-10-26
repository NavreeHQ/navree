//! [`Cache`]: the typed cache API, and [`Ttl`].

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use navis_kernel::BoxError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio_util::task::TaskTracker;

use crate::flight::Flights;
use crate::{CacheError, CacheStore, MemoryStore};

/// The default TTL of [`Cache::memory`] and of `CacheOptions` (one hour).
pub const DEFAULT_TTL: Duration = Duration::from_secs(3600);

/// How long a cached value lives.
///
/// A plain [`Duration`] converts into [`Ttl::For`], so most calls read
/// `cache.put("k", &v, Duration::from_secs(60))`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Ttl {
    /// The cache's default TTL (`default_ttl_secs`, one hour unless configured).
    #[default]
    Default,
    /// Never expires (it can still be forgotten or flushed).
    Forever,
    /// Expires after this long. [`Duration::ZERO`] removes the key instead of storing it.
    For(Duration),
}

impl From<Duration> for Ttl {
    fn from(ttl: Duration) -> Self {
        Ttl::For(ttl)
    }
}

/// The application cache: JSON-encoded values in a [`CacheStore`] under a
/// key prefix, with `remember`, stale-while-revalidate
/// ([`flexible`](Cache::flexible)), [tags](Cache::tags) and counters.
///
/// Provided (and exported globally) by `CacheModule`; cheap to clone.
///
/// ```
/// use std::time::Duration;
/// use navis_cache::Cache;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navis_cache::CacheError> {
/// let cache = Cache::memory();
/// let total: u64 = cache
///     .remember("stats:total", Duration::from_secs(60), || async {
///         Ok(42) // an expensive query
///     })
///     .await?;
/// assert_eq!(cache.get::<u64>("stats:total").await?, Some(total));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Cache {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<dyn CacheStore>,
    prefix: String,
    default_ttl: Duration,
    flights: Flights,
    /// Where background refreshes are spawned (the app's tracker, so graceful
    /// shutdown waits for them); `tokio::spawn` when `None`.
    tracker: Option<TaskTracker>,
}

/// A key as the caller wrote it (for errors) and as the store sees it.
pub(crate) struct Slot<'a> {
    pub(crate) key: &'a str,
    pub(crate) full: String,
}

impl Cache {
    /// A cache over `store`. Every key is stored as `prefix + key`;
    /// [`Ttl::Default`] means `default_ttl`.
    pub fn new(
        store: Arc<dyn CacheStore>,
        prefix: impl Into<String>,
        default_ttl: Duration,
    ) -> Self {
        Self::with_tracker(store, prefix.into(), default_ttl, None)
    }

    /// A cache over a fresh [`MemoryStore`], no prefix, one-hour default TTL.
    /// Handy in tests and scripts; apps get their `Cache` from `CacheModule`.
    pub fn memory() -> Self {
        Self::new(Arc::new(MemoryStore::new()), "", DEFAULT_TTL)
    }

    pub(crate) fn with_tracker(
        store: Arc<dyn CacheStore>,
        prefix: String,
        default_ttl: Duration,
        tracker: Option<TaskTracker>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                prefix,
                default_ttl,
                flights: Flights::default(),
                tracker,
            }),
        }
    }

    /// The underlying store.
    pub fn store(&self) -> &Arc<dyn CacheStore> {
        &self.inner.store
    }

    /// The prefix added to every key.
    pub fn prefix(&self) -> &str {
        &self.inner.prefix
    }

    /// The TTL used for [`Ttl::Default`].
    pub fn default_ttl(&self) -> Duration {
        self.inner.default_ttl
    }

    /// The value under `key`, or `None` if it is missing or expired.
    /// Fails with [`CacheError::Deserialize`] if the stored value is not a `T`.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        self.get_slot(&self.slot(key)).await
    }

    /// Stores `value` under `key` for `ttl`.
    pub async fn put<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        ttl: impl Into<Ttl>,
    ) -> Result<(), CacheError> {
        let ttl = self.resolve(ttl.into());
        self.put_slot(&self.slot(key), value, ttl).await
    }

    /// Stores `value` under `key` with no expiry.
    pub async fn forever<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), CacheError> {
        self.put(key, value, Ttl::Forever).await
    }

    /// Stores `value` only if `key` has no live value (atomic in the store);
    /// `true` if it was stored. A zero TTL stores nothing and returns `false`.
    pub async fn add<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        ttl: impl Into<Ttl>,
    ) -> Result<bool, CacheError> {
        let ttl = self.resolve(ttl.into());
        self.add_slot(&self.slot(key), value, ttl).await
    }

    /// Whether `key` has a live value.
    pub async fn has(&self, key: &str) -> Result<bool, CacheError> {
        Ok(self.inner.store.get(&self.full_key(key)).await?.is_some())
    }

    /// Removes `key`; `true` if a live value was removed.
    pub async fn forget(&self, key: &str) -> Result<bool, CacheError> {
        self.inner.store.forget(&self.full_key(key)).await
    }

    /// Returns the value under `key` and removes it.
    pub async fn pull<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        let slot = self.slot(key);
        let value = self.get_slot(&slot).await?;
        self.inner.store.forget(&slot.full).await?;
        Ok(value)
    }

    /// Adds `by` to the counter under `key` (missing counts as 0) and returns
    /// the new value; the entry keeps its TTL. Read it back with `get::<i64>`.
    pub async fn increment(&self, key: &str, by: i64) -> Result<i64, CacheError> {
        self.inner.store.increment(&self.full_key(key), by).await
    }

    /// Subtracts `by` from the counter under `key` (see [`increment`](Cache::increment)).
    pub async fn decrement(&self, key: &str, by: i64) -> Result<i64, CacheError> {
        let full = self.full_key(key);
        let by = negate(&full, by)?;
        self.inner.store.increment(&full, by).await
    }

    /// The value under `key`; on a miss, computes it with `f`, stores it for
    /// `ttl` and returns it.
    ///
    /// Concurrent callers for the same key in this process wait for one
    /// computation instead of all running `f`. An error from `f` is returned
    /// as [`CacheError::Compute`] and nothing is cached. A stored value that
    /// no longer decodes as `T` (for example after a deploy changed the type)
    /// counts as a miss and is replaced.
    pub async fn remember<T, F, Fut>(
        &self,
        key: &str,
        ttl: impl Into<Ttl>,
        f: F,
    ) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, BoxError>>,
    {
        let ttl = self.resolve(ttl.into());
        self.remember_slot(self.slot(key), ttl, f).await
    }

    /// [`remember`](Cache::remember) with no expiry.
    pub async fn remember_forever<T, F, Fut>(&self, key: &str, f: F) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, BoxError>>,
    {
        self.remember(key, Ttl::Forever, f).await
    }

    /// Stale-while-revalidate: the value under `key` is *fresh* for `fresh`
    /// after it was computed, then *stale* for a further `stale`.
    ///
    /// - fresh: returned as is;
    /// - stale: returned at once, and `f` refreshes it in a background task
    ///   (one refresh at a time per key, also across processes sharing the
    ///   store; a failed refresh is logged and the stale value kept);
    /// - missing (or past both windows): computed with `f` before returning,
    ///   like [`remember`](Cache::remember).
    ///
    /// Inside an app, background refreshes run on the app's task tracker, so
    /// graceful shutdown waits for them.
    ///
    /// ```
    /// use std::time::Duration;
    /// use navis_cache::Cache;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), navis_cache::CacheError> {
    /// let cache = Cache::memory();
    /// let fresh = Duration::from_secs(60);
    /// let stale = Duration::from_secs(300);
    /// let rates: Vec<f64> = cache
    ///     .flexible("fx:rates", fresh, stale, || async { Ok(vec![1.0, 0.92]) })
    ///     .await?;
    /// assert_eq!(rates.len(), 2);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn flexible<T, F, Fut>(
        &self,
        key: &str,
        fresh: Duration,
        stale: Duration,
        f: F,
    ) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static,
    {
        self.flexible_slot(self.slot(key), fresh, stale, f).await
    }

    /// Removes every entry of the store.
    pub async fn flush(&self) -> Result<(), CacheError> {
        self.inner.store.flush().await
    }

    // ----- shared with `TaggedCache` -------------------------------------

    pub(crate) fn full_key(&self, key: &str) -> String {
        format!("{}{key}", self.inner.prefix)
    }

    fn slot<'a>(&self, key: &'a str) -> Slot<'a> {
        Slot {
            key,
            full: self.full_key(key),
        }
    }

    /// The store TTL for `ttl` (`None` = no expiry).
    pub(crate) fn resolve(&self, ttl: Ttl) -> Option<Duration> {
        match ttl {
            Ttl::Default => Some(self.inner.default_ttl),
            Ttl::Forever => None,
            Ttl::For(ttl) => Some(ttl),
        }
    }

    pub(crate) async fn get_slot<T: DeserializeOwned>(
        &self,
        slot: &Slot<'_>,
    ) -> Result<Option<T>, CacheError> {
        match self.inner.store.get(&slot.full).await? {
            Some(bytes) => decode(slot.key, &bytes).map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn put_slot<T: Serialize + ?Sized>(
        &self,
        slot: &Slot<'_>,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError> {
        if ttl == Some(Duration::ZERO) {
            self.inner.store.forget(&slot.full).await?;
            return Ok(());
        }
        let bytes = encode(slot.key, value)?;
        self.inner.store.put(&slot.full, bytes, ttl).await
    }

    pub(crate) async fn add_slot<T: Serialize + ?Sized>(
        &self,
        slot: &Slot<'_>,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<bool, CacheError> {
        if ttl == Some(Duration::ZERO) {
            return Ok(false);
        }
        let bytes = encode(slot.key, value)?;
        self.inner.store.add(&slot.full, bytes, ttl).await
    }

    pub(crate) async fn remember_slot<T, F, Fut>(
        &self,
        slot: Slot<'_>,
        ttl: Option<Duration>,
        f: F,
    ) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, BoxError>>,
    {
        if let Some(value) = self.read_lenient(&slot).await? {
            return Ok(value);
        }
        let _flight = self.inner.flights.enter(&slot.full).await;
        // Another caller may have computed it while we waited.
        if let Some(value) = self.read_lenient(&slot).await? {
            return Ok(value);
        }
        let value = f().await.map_err(|source| CacheError::Compute {
            key: slot.key.to_owned(),
            source,
        })?;
        self.put_slot(&slot, &value, ttl).await?;
        Ok(value)
    }

    pub(crate) async fn flexible_slot<T, F, Fut>(
        &self,
        slot: Slot<'_>,
        fresh: Duration,
        stale: Duration,
        f: F,
    ) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static,
    {
        let windows = Windows { fresh, stale };
        if let Some(value) = self.read_lenient(&slot).await? {
            if self
                .inner
                .store
                .get(&fresh_marker(&slot.full))
                .await?
                .is_none()
            {
                self.refresh_in_background(&slot, windows, f).await?;
            }
            return Ok(value);
        }
        let _flight = self.inner.flights.enter(&slot.full).await;
        if let Some(value) = self.read_lenient(&slot).await? {
            return Ok(value);
        }
        let value = f().await.map_err(|source| CacheError::Compute {
            key: slot.key.to_owned(),
            source,
        })?;
        let bytes = encode(slot.key, &value)?;
        windows
            .store(self.inner.store.as_ref(), &slot.full, bytes)
            .await?;
        Ok(value)
    }

    /// Spawns `f` to refresh a stale `slot`, unless a refresh is already running.
    async fn refresh_in_background<T, F, Fut>(
        &self,
        slot: &Slot<'_>,
        windows: Windows,
        f: F,
    ) -> Result<(), CacheError>
    where
        T: Serialize + Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, BoxError>> + Send + 'static,
    {
        let store = self.inner.store.clone();
        let claim = refresh_claim(&slot.full);
        // `add` is atomic, so one refresh runs per key (also across processes);
        // the TTL frees the claim if this process dies mid-refresh.
        let claim_ttl = windows
            .stale
            .clamp(Duration::from_secs(1), REFRESH_CLAIM_MAX);
        if !store
            .add(&claim, Bytes::from_static(b"1"), Some(claim_ttl))
            .await?
        {
            return Ok(());
        }
        let key = slot.key.to_owned();
        let full = slot.full.clone();
        let refresh = async move {
            let outcome: Result<(), BoxError> = match f().await {
                Ok(value) => match encode(&key, &value) {
                    Ok(bytes) => windows
                        .store(store.as_ref(), &full, bytes)
                        .await
                        .map_err(Into::into),
                    Err(error) => Err(error.into()),
                },
                Err(error) => Err(error),
            };
            if let Err(error) = outcome {
                tracing::warn!(
                    key = %key,
                    error = %error,
                    "background cache refresh failed; serving the stale value until it expires"
                );
            }
            if let Err(error) = store.forget(&claim).await {
                tracing::warn!(key = %key, error = %error, "could not release the refresh claim");
            }
        };
        match &self.inner.tracker {
            Some(tracker) => drop(tracker.spawn(refresh)),
            None => drop(tokio::spawn(refresh)),
        }
        Ok(())
    }

    /// Like [`get_slot`](Cache::get_slot), but a value that does not decode is a miss.
    async fn read_lenient<T: DeserializeOwned>(
        &self,
        slot: &Slot<'_>,
    ) -> Result<Option<T>, CacheError> {
        match self.get_slot(slot).await {
            Err(CacheError::Deserialize { key, .. }) => {
                tracing::warn!(key = %key, "cached value could not be decoded; recomputing it");
                Ok(None)
            }
            other => other,
        }
    }
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache")
            .field("prefix", &self.inner.prefix)
            .field("default_ttl", &self.inner.default_ttl)
            .finish_non_exhaustive()
    }
}

/// The longest a background refresh holds its claim before another may start.
const REFRESH_CLAIM_MAX: Duration = Duration::from_secs(60);

/// The fresh and stale windows of a [`Cache::flexible`] entry.
#[derive(Clone, Copy)]
struct Windows {
    fresh: Duration,
    stale: Duration,
}

impl Windows {
    /// Stores the value for `fresh + stale` and a marker that lives for
    /// `fresh`: the value is stale once the marker is gone. Expiry is left to
    /// the store, so no clock is involved.
    async fn store(
        self,
        store: &dyn CacheStore,
        full: &str,
        value: Bytes,
    ) -> Result<(), CacheError> {
        let marker = fresh_marker(full);
        let lifetime = self.fresh.saturating_add(self.stale);
        if lifetime.is_zero() {
            store.forget(full).await?;
        } else {
            store.put(full, value, Some(lifetime)).await?;
        }
        if self.fresh.is_zero() {
            store.forget(&marker).await?;
        } else {
            store
                .put(&marker, Bytes::from_static(b"1"), Some(self.fresh))
                .await?;
        }
        Ok(())
    }
}

/// The key whose presence means the `flexible` entry at `full` is fresh.
fn fresh_marker(full: &str) -> String {
    format!("{full}#navis:fresh")
}

/// The key claimed while a background refresh of `full` runs.
fn refresh_claim(full: &str) -> String {
    format!("{full}#navis:refresh")
}

/// `-by` for `decrement`; overflows only for `i64::MIN`.
pub(crate) fn negate(full: &str, by: i64) -> Result<i64, CacheError> {
    by.checked_neg().ok_or_else(|| CacheError::Overflow {
        key: full.to_owned(),
    })
}

fn encode<T: Serialize + ?Sized>(key: &str, value: &T) -> Result<Bytes, CacheError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|source| CacheError::Serialize {
            key: key.to_owned(),
            source,
        })
}

fn decode<T: DeserializeOwned>(key: &str, bytes: &[u8]) -> Result<T, CacheError> {
    serde_json::from_slice(bytes).map_err(|source| CacheError::Deserialize {
        key: key.to_owned(),
        source,
    })
}
