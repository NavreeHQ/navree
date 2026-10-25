//! [`CacheError`]: everything a cache, store or lock operation can fail with.

use std::time::Duration;

use keel_kernel::BoxError;

/// An error from a [`Cache`](crate::Cache), a [`CacheStore`](crate::CacheStore)
/// or a [`Lock`](crate::Lock).
///
/// Messages name the key (or lock) involved, never the cached value.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CacheError {
    /// The value could not be encoded as JSON.
    #[error("the value for cache key `{key}` could not be serialized")]
    Serialize {
        /// The cache key.
        key: String,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },

    /// The stored bytes could not be decoded into the requested type.
    #[error("the value stored under cache key `{key}` could not be deserialized")]
    Deserialize {
        /// The cache key.
        key: String,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },

    /// `increment` found a value that is not an integer.
    #[error("the value stored under cache key `{key}` is not an integer")]
    NotAnInteger {
        /// The store key (prefix included).
        key: String,
    },

    /// `increment` or `decrement` would overflow an `i64`; the stored
    /// value is unchanged.
    #[error("incrementing cache key `{key}` would overflow")]
    Overflow {
        /// The store key (prefix included).
        key: String,
    },

    /// The closure passed to `remember` or `flexible` failed; nothing was cached.
    #[error("computing the value for cache key `{key}` failed")]
    Compute {
        /// The cache key.
        key: String,
        /// The closure's error.
        #[source]
        source: BoxError,
    },

    /// [`Lock::block`](crate::Lock::block) gave up waiting.
    #[error("timed out after {waited:?} waiting for lock `{name}`")]
    LockTimeout {
        /// The lock name.
        name: String,
        /// How long it waited.
        waited: Duration,
    },

    /// The store backend failed (connection, protocol, ...).
    #[error("the cache store failed")]
    Store(#[source] BoxError),
}
