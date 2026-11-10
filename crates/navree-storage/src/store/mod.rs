//! The file stores behind a disk: Navree's own for the local directory and
//! memory, an adapter for each cloud provider, and the key prefix a disk
//! can sit under.
//!
//! A store works on normalised paths (see [`normalize`](crate::path::normalize)):
//! `/`-separated, no empty, `.` or `..` segments, no control characters.
//! Every method is the whole operation; nothing here retries or caches.

use std::fmt;
use std::ops::Range;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use navree_kernel::{BoxError, BoxFuture};

#[cfg(feature = "__cloud")]
pub(crate) mod cloud;
mod local;
mod memory;
mod prefixed;

pub(crate) use local::Local;
pub(crate) use memory::Memory;
pub(crate) use prefixed::Prefixed;

/// What a store reports about a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Meta {
    /// The path, relative to the store.
    pub(crate) path: String,
    /// The size in bytes.
    pub(crate) size: u64,
    /// When the file was last written.
    pub(crate) last_modified: DateTime<Utc>,
}

/// A file opened for streaming.
pub(crate) struct Object {
    /// The size of the whole file in bytes.
    pub(crate) size: u64,
    /// The contents.
    pub(crate) stream: BoxStream<'static, StoreResult<Bytes>>,
}

/// How a store operation fails.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    /// There is no such file.
    #[error("not found")]
    NotFound,
    /// The store could not do it.
    #[error("{0}")]
    Failed(BoxError),
}

impl StoreError {
    /// A failure with `message`.
    pub(crate) fn failed(message: impl Into<BoxError>) -> Self {
        Self::Failed(message.into())
    }
}

pub(crate) type StoreResult<T> = Result<T, StoreError>;

/// A file store.
pub(crate) trait Store: fmt::Display + Send + Sync {
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
}

/// A file being written.
pub(crate) trait Upload: Send {
    /// Appends `chunk`.
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>>;

    /// Publishes the file.
    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>>;

    /// Drops what was written.
    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>>;
}
