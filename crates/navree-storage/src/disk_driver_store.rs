//! [`DriverStore`]: a disk served by a [`DiskDriver`] a module registers,
//! behind the same [`Store`] every disk uses, so paths are normalised and
//! links signed before the driver is reached.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::TryStreamExt;
use navree_kernel::BoxFuture;

use crate::FileMeta;
use crate::driver::{DiskDriver, DiskDriverError, DiskUpload, OpenedFile};
use crate::store::{Meta, Object, Store, StoreError, StoreResult, Upload};

/// A registered driver as a disk's store.
pub(crate) struct DriverStore(pub(crate) Arc<dyn DiskDriver>);

/// The store's error for a driver's: not found stays not found.
fn store_error(error: DiskDriverError) -> StoreError {
    match error {
        DiskDriverError::NotFound => StoreError::NotFound,
        DiskDriverError::Failed(error) => StoreError::Failed(error),
    }
}

/// The store's metadata for a driver's.
fn meta(file: FileMeta) -> Meta {
    Meta {
        path: file.path,
        size: file.size,
        last_modified: file.last_modified,
    }
}

/// The driver's own description.
impl fmt::Display for DriverStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.describe())
    }
}

impl Store for DriverStore {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        let put = self.0.put(path, contents);
        Box::pin(async move { put.await.map_err(store_error) })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        let begun = self.0.begin_upload(path);
        Box::pin(async move {
            let upload = begun.await.map_err(store_error)?;
            Ok(Box::new(DriverUpload(upload)) as Box<dyn Upload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        let read = self.0.read(path, range);
        Box::pin(async move { read.await.map_err(store_error) })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        let opened = self.0.open(path);
        Box::pin(async move {
            let OpenedFile { size, stream } = opened.await.map_err(store_error)?;
            Ok(Object {
                size,
                stream: Box::pin(stream.map_err(store_error)),
            })
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        let head = self.0.head(path);
        Box::pin(async move { head.await.map(meta).map_err(store_error) })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        let delete = self.0.delete(path);
        Box::pin(async move { delete.await.map_err(store_error) })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        let list = self.0.list(prefix);
        Box::pin(async move {
            let files = list.await.map_err(store_error)?;
            Ok(files.into_iter().map(meta).collect())
        })
    }
}

/// A driver's upload as a store's.
struct DriverUpload(Box<dyn DiskUpload>);

impl Upload for DriverUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        let written = self.0.write(chunk);
        Box::pin(async move { written.await.map_err(store_error) })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        let DriverUpload(upload) = *self;
        let finished = upload.finish();
        Box::pin(async move { finished.await.map_err(store_error) })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        let DriverUpload(upload) = *self;
        let aborted = upload.abort();
        Box::pin(async move { aborted.await.map_err(store_error) })
    }
}
