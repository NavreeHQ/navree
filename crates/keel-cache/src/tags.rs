//! [`TaggedCache`]: entries grouped under tags and invalidated together.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use keel_kernel::BoxError;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::cache::{Slot, negate};
use crate::{Cache, CacheError, Ttl};

/// A view of a [`Cache`] whose entries belong to a set of tags, created by
/// [`Cache::tags`]. [`flush`](TaggedCache::flush) invalidates every entry
/// stored under any of these tags and leaves everything else alone.
///
/// Each tag has a random version stored in the cache (`keel:tag:<tag>`); an
/// entry's key includes the versions of all its tags, so flushing a tag
/// (giving it a new version) makes its entries unreachable. They then expire
/// with their TTL, so prefer TTLs over `forever` for tagged entries.
///
/// The tag set is order-insensitive: `tags(&["a", "b"])` and
/// `tags(&["b", "a"])` address the same entries, while `tags(&["a"])` and the
/// untagged cache do not.
///
/// ```
/// use std::time::Duration;
/// use keel_cache::Cache;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), keel_cache::CacheError> {
/// let cache = Cache::memory();
/// let users = cache.tags(&["users"]);
/// users.put("count", &3, Duration::from_secs(60)).await?;
/// users.flush().await?;
/// assert_eq!(users.get::<u32>("count").await?, None);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TaggedCache {
    cache: Cache,
    tags: Vec<String>,
}

impl Cache {
    /// A view of this cache whose entries are grouped under `tags`.
    pub fn tags(&self, tags: &[&str]) -> TaggedCache {
        let mut tags: Vec<String> = tags.iter().map(|&t| t.to_owned()).collect();
        tags.sort();
        tags.dedup();
        TaggedCache {
            cache: self.clone(),
            tags,
        }
    }
}

impl TaggedCache {
    /// The tags, sorted and deduplicated.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// See [`Cache::get`].
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        self.cache.get_slot(&self.slot(key).await?).await
    }

    /// See [`Cache::put`].
    pub async fn put<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        ttl: impl Into<Ttl>,
    ) -> Result<(), CacheError> {
        let ttl = self.cache.resolve(ttl.into());
        self.cache
            .put_slot(&self.slot(key).await?, value, ttl)
            .await
    }

    /// See [`Cache::forever`].
    pub async fn forever<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), CacheError> {
        self.put(key, value, Ttl::Forever).await
    }

    /// See [`Cache::add`].
    pub async fn add<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        ttl: impl Into<Ttl>,
    ) -> Result<bool, CacheError> {
        let ttl = self.cache.resolve(ttl.into());
        self.cache
            .add_slot(&self.slot(key).await?, value, ttl)
            .await
    }

    /// See [`Cache::has`].
    pub async fn has(&self, key: &str) -> Result<bool, CacheError> {
        let slot = self.slot(key).await?;
        Ok(self.cache.store().get(&slot.full).await?.is_some())
    }

    /// See [`Cache::forget`].
    pub async fn forget(&self, key: &str) -> Result<bool, CacheError> {
        let slot = self.slot(key).await?;
        self.cache.store().forget(&slot.full).await
    }

    /// See [`Cache::increment`].
    pub async fn increment(&self, key: &str, by: i64) -> Result<i64, CacheError> {
        let slot = self.slot(key).await?;
        self.cache.store().increment(&slot.full, by).await
    }

    /// See [`Cache::decrement`].
    pub async fn decrement(&self, key: &str, by: i64) -> Result<i64, CacheError> {
        let slot = self.slot(key).await?;
        let by = negate(&slot.full, by)?;
        self.cache.store().increment(&slot.full, by).await
    }

    /// See [`Cache::remember`].
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
        let ttl = self.cache.resolve(ttl.into());
        self.cache
            .remember_slot(self.slot(key).await?, ttl, f)
            .await
    }

    /// See [`Cache::remember_forever`].
    pub async fn remember_forever<T, F, Fut>(&self, key: &str, f: F) -> Result<T, CacheError>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, BoxError>>,
    {
        self.remember(key, Ttl::Forever, f).await
    }

    /// See [`Cache::flexible`].
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
        self.cache
            .flexible_slot(self.slot(key).await?, fresh, stale, f)
            .await
    }

    /// Invalidates every entry stored under any of these tags.
    pub async fn flush(&self) -> Result<(), CacheError> {
        for tag in &self.tags {
            self.cache
                .store()
                .put(&self.tag_key(tag), new_version(), None)
                .await?;
        }
        Ok(())
    }

    /// The store key of `key` under the current versions of the tags.
    pub(crate) async fn slot<'a>(&self, key: &'a str) -> Result<Slot<'a>, CacheError> {
        let mut versions = Vec::with_capacity(self.tags.len());
        for tag in &self.tags {
            versions.push(self.version(tag).await?);
        }
        Ok(Slot {
            key,
            full: self
                .cache
                .full_key(&format!("keel:tagged:{}:{key}", versions.join("|"))),
        })
    }

    /// The current version of `tag`, created on first use (atomically, so
    /// concurrent first uses agree).
    async fn version(&self, tag: &str) -> Result<String, CacheError> {
        let store = self.cache.store();
        let key = self.tag_key(tag);
        if let Some(version) = store.get(&key).await? {
            return Ok(String::from_utf8_lossy(&version).into_owned());
        }
        let fresh = new_version();
        store.add(&key, fresh.clone(), None).await?;
        let version = store.get(&key).await?.unwrap_or(fresh);
        Ok(String::from_utf8_lossy(&version).into_owned())
    }

    fn tag_key(&self, tag: &str) -> String {
        self.cache.full_key(&format!("keel:tag:{tag}"))
    }
}

impl fmt::Debug for TaggedCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaggedCache")
            .field("cache", &self.cache)
            .field("tags", &self.tags)
            .finish()
    }
}

/// A random tag version.
fn new_version() -> Bytes {
    Bytes::from(uuid::Uuid::new_v4().simple().to_string())
}
