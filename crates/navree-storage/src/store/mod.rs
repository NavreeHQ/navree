//! The file store behind a disk, and what a driver of one implements:
//! [`Store`], with its [`Upload`], [`Meta`], [`Object`] and [`StoreError`].
//! [`MemoryStore`] keeps its files in memory, and [`PrefixedStore`] is a
//! store seen from one directory of another. The local directory and each
//! cloud provider have a store of their own.
//!
//! A store works on normalised paths: `/`-separated, no empty, `.` or `..`
//! segments, no control characters. A [`Disk`](crate::Disk) normalises every
//! path before its store sees it. Every method is the whole operation: a
//! disk does not try again or cache on top of its store.
//!
//! ```
//! use std::sync::Arc;
//!
//! use bytes::Bytes;
//! use navree_storage::store::{MemoryStore, PrefixedStore, Store, StoreError};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), StoreError> {
//! let files = Arc::new(MemoryStore::new());
//! let agra = PrefixedStore::new(files.clone(), "tenants/agra");
//! agra.put("invoices/INV-1101.csv", Bytes::from("total\n100000\n")).await?;
//!
//! assert_eq!(files.head("tenants/agra/invoices/INV-1101.csv").await?.size, 13);
//! assert_eq!(agra.list("invoices").await?[0].path, "invoices/INV-1101.csv");
//! assert!(matches!(
//!     agra.read("missing.csv", None).await,
//!     Err(StoreError::NotFound)
//! ));
//! # Ok(())
//! # }
//! ```

use std::fmt;
use std::ops::Range;
use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use navree_kernel::{BoxError, BoxFuture};

#[cfg(feature = "__cloud")]
pub(crate) mod cloud;
mod local;
mod memory;
mod prefixed;

/// The method of a [signed link](Store::signed_url): `GET` or `PUT`.
pub use http::Method;
/// A [signed link](Store::signed_url).
pub use url::Url;

pub(crate) use local::Local;
pub use memory::MemoryStore;
pub use prefixed::PrefixedStore;

/// What a store reports about a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// The path, relative to the store.
    pub path: String,
    /// The size in bytes.
    pub size: u64,
    /// When the file was last written.
    pub last_modified: DateTime<Utc>,
}

/// A file opened for streaming.
///
/// ```
/// use bytes::Bytes;
/// use futures_util::stream;
/// use navree_storage::store::Object;
///
/// let contents = Bytes::from_static(b"invoice,total\nINV-1101,100000\n");
/// let object = Object::new(
///     contents.len() as u64,
///     Box::pin(stream::once(async move { Ok(contents) })),
/// );
/// assert_eq!(object.size, 30);
/// ```
#[non_exhaustive]
pub struct Object {
    /// The size of the whole file in bytes.
    pub size: u64,
    /// The contents.
    pub stream: BoxStream<'static, StoreResult<Bytes>>,
}

impl Object {
    /// A file of `size` bytes whose contents are `stream`.
    pub fn new(size: u64, stream: BoxStream<'static, StoreResult<Bytes>>) -> Object {
        Object { size, stream }
    }
}

/// The size; never the contents.
impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// How a store operation fails.
///
/// ```
/// use navree_storage::store::StoreError;
///
/// assert_eq!(StoreError::NotFound.to_string(), "not found");
/// let error = StoreError::failed("the Ludhiana bucket refused the write");
/// assert_eq!(error.to_string(), "the Ludhiana bucket refused the write");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// There is no such file.
    #[error("not found")]
    NotFound,
    /// The store has no such operation: a [signed link](Store::signed_url)
    /// asked of a store that signs none. It names what was asked for.
    #[error("this store has no {0}")]
    Unsupported(&'static str),
    /// The store could not do it; the error says why, without the file's
    /// contents, a password or a token.
    #[error("{0}")]
    Failed(BoxError),
}

impl StoreError {
    /// A failure with `message`.
    pub fn failed(message: impl Into<BoxError>) -> Self {
        Self::Failed(message.into())
    }
}

/// What a store operation gives.
pub type StoreResult<T> = Result<T, StoreError>;

/// A file store.
pub trait Store: fmt::Display + Send + Sync {
    /// Writes `contents` to `path`, replacing any file there; readers see
    /// the old file or the new one, never part of one.
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>>;

    /// Starts writing a file chunk by chunk. The file appears at `path`
    /// only when the upload is [finished](Upload::finish).
    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>>;

    /// The bytes of `path`, or of `range` of it (cut short at the end of
    /// the file).
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>>;

    /// Opens `path` for streaming.
    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>>;

    /// The size and modification time of `path`.
    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>>;

    /// Deletes `path`; [`StoreError::NotFound`] when there is none.
    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>>;

    /// Every file under the directory `prefix` (`""` is the root), in no
    /// particular order. A directory that does not exist has none.
    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>>;

    /// A link that lets whoever holds it download (`GET`) or upload (`PUT`)
    /// the file at `path` for `expires_in`, signed by the store, so the
    /// bytes travel between the client and the store and never through the
    /// app. The link carries its own expiry; nothing is kept to take it
    /// back.
    ///
    /// Only a store that can sign gives one: a cloud provider's. Every
    /// other store keeps this default, which fails with
    /// [`StoreError::Unsupported`]. A store refuses a method it does not
    /// sign and a lifetime longer than its provider allows with
    /// [`StoreError::Failed`]. Headers a link is bound to (a content type,
    /// a size range) are a method of the store that can bind them, not of
    /// this trait.
    fn signed_url<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        expires_in: Duration,
    ) -> BoxFuture<'a, StoreResult<Url>> {
        let _ = (path, method, expires_in);
        Box::pin(async { Err(StoreError::Unsupported("signed links")) })
    }
}

/// A file being written.
pub trait Upload: Send {
    /// Appends `chunk`.
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>>;

    /// Publishes the file.
    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>>;

    /// Drops what was written.
    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>>;
}
