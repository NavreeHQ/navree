//! [`Cloud`]: a bucket of S3, Google Cloud Storage or Azure, through the
//! provider's `object_store` client.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use navree_kernel::BoxFuture;
use object_store::path::Path as ObjectPath;
use object_store::{GetOptions, GetRange, ObjectMeta, ObjectStore, ObjectStoreExt, WriteMultipart};

use super::{Meta, Object, Store, StoreError, StoreResult, Upload};

/// Parts uploaded concurrently by an [`Upload`].
const UPLOAD_CONCURRENCY: usize = 2;

/// A provider's bucket.
pub(crate) struct Cloud {
    store: Arc<dyn ObjectStore>,
}

impl Cloud {
    pub(crate) fn new(store: Arc<dyn ObjectStore>) -> Self {
        Self { store }
    }
}

impl fmt::Display for Cloud {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cloud({})", self.store)
    }
}

/// The client's error as a [`StoreError`].
fn convert(error: object_store::Error) -> StoreError {
    match error {
        object_store::Error::NotFound { .. } => StoreError::NotFound,
        other => StoreError::Failed(Box::new(other)),
    }
}

/// The client's name for the normalised `path`.
fn location(path: &str) -> StoreResult<ObjectPath> {
    ObjectPath::parse(path).map_err(StoreError::failed)
}

fn meta(meta: ObjectMeta) -> Meta {
    Meta {
        path: meta.location.as_ref().to_owned(),
        size: meta.size,
        last_modified: meta.last_modified,
    }
}

impl Store for Cloud {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let location = location(path)?;
            self.store
                .put(&location, contents.into())
                .await
                .map(drop)
                .map_err(convert)
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            let location = location(path)?;
            let upload = self.store.put_multipart(&location).await.map_err(convert)?;
            Ok(Box::new(CloudUpload {
                writer: WriteMultipart::new(upload),
            }) as Box<dyn Upload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let location = location(path)?;
            let options = GetOptions {
                range: range.map(GetRange::Bounded),
                ..GetOptions::default()
            };
            self.store
                .get_opts(&location, options)
                .await
                .map_err(convert)?
                .bytes()
                .await
                .map_err(convert)
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let location = location(path)?;
            let result = self.store.get(&location).await.map_err(convert)?;
            Ok(Object {
                size: result.meta.size,
                stream: result.into_stream().map_err(convert).boxed(),
            })
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let location = location(path)?;
            self.store.head(&location).await.map(meta).map_err(convert)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let location = location(path)?;
            self.store.delete(&location).await.map_err(convert)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            let prefix = location(prefix)?;
            self.store
                .list(Some(&prefix))
                .map_ok(meta)
                .try_collect()
                .await
                .map_err(convert)
        })
    }
}

/// A multipart upload.
struct CloudUpload {
    writer: WriteMultipart,
}

impl Upload for CloudUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            self.writer
                .wait_for_capacity(UPLOAD_CONCURRENCY)
                .await
                .map_err(convert)?;
            self.writer.put(chunk);
            Ok(())
        })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            let CloudUpload { writer } = *self;
            writer.finish().await.map(drop).map_err(convert)
        })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            let CloudUpload { writer } = *self;
            writer.abort().await.map_err(convert)
        })
    }
}
