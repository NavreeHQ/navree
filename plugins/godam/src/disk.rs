//! [`GodamDisk`]: a [`DiskDriver`] backed by an `object_store` store.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use navree::kernel::BoxFuture;
use navree::prelude::BoxError;
use navree::storage::{DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile};
use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::prefix::PrefixStore;
use object_store::{MultipartUpload, ObjectMeta, ObjectStore, ObjectStoreExt};

use crate::GodamSettings;

/// A [`DiskDriver`] over an [`ObjectStore`], built by default as
/// `PrefixStore` around [`InMemory`].
#[derive(Clone)]
pub struct GodamDisk {
    store: Arc<dyn ObjectStore>,
}

impl fmt::Debug for GodamDisk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GodamDisk").finish_non_exhaustive()
    }
}

impl GodamDisk {
    /// Builds an in-memory `godam` disk scoped to `settings.prefix`.
    pub fn new(settings: GodamSettings) -> Result<GodamDisk, BoxError> {
        let prefix = settings.parse_prefix(None)?;
        Ok(GodamDisk {
            store: Arc::new(PrefixStore::new(InMemory::new(), prefix)),
        })
    }

    /// Wraps an existing [`ObjectStore`] directly as a `godam` disk.
    pub fn from_store(store: Arc<dyn ObjectStore>) -> GodamDisk {
        GodamDisk { store }
    }

    /// Wraps an existing [`ObjectStore`] under `settings.prefix`.
    pub fn with_store(
        store: Arc<dyn ObjectStore>,
        settings: GodamSettings,
    ) -> Result<GodamDisk, BoxError> {
        let prefix = settings.parse_prefix(None)?;
        Ok(GodamDisk {
            store: Arc::new(PrefixStore::new(store, prefix)),
        })
    }
}

fn operation_failed(operation: &'static str) -> DiskDriverError {
    DiskDriverError::Failed(format!("godam {operation} failed").into())
}

fn driver_error(operation: &'static str, error: object_store::Error) -> DiskDriverError {
    match error {
        object_store::Error::NotFound { .. } => DiskDriverError::NotFound,
        _ => operation_failed(operation),
    }
}

fn file_location(path: &str, operation: &'static str) -> Result<ObjectPath, DiskDriverError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return Err(operation_failed(operation));
    }
    ObjectPath::parse(path).map_err(|_| operation_failed(operation))
}

fn list_location(prefix: &str) -> Result<Option<ObjectPath>, DiskDriverError> {
    if prefix.is_empty() {
        return Ok(None);
    }
    file_location(prefix, "list").map(Some)
}

impl DiskDriver for GodamDisk {
    fn describe(&self) -> String {
        "godam disk in memory".to_owned()
    }

    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: Bytes,
    ) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "put")?;
            self.store
                .put(&location, contents.into())
                .await
                .map(drop)
                .map_err(|error| driver_error("put", error))
        })
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "upload")?;
            let upload = self
                .store
                .put_multipart(&location)
                .await
                .map_err(|error| driver_error("upload", error))?;
            Ok(Box::new(GodamUpload {
                upload: Some(upload),
            }) as Box<dyn DiskUpload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "read")?;
            match range {
                None => self
                    .store
                    .get(&location)
                    .await
                    .map_err(|error| driver_error("read", error))?
                    .bytes()
                    .await
                    .map_err(|error| driver_error("read", error)),
                Some(range) => {
                    let meta = self
                        .store
                        .head(&location)
                        .await
                        .map_err(|error| driver_error("read", error))?;
                    let end = range.end.min(meta.size);
                    let start = range.start.min(end);
                    if start == end {
                        return Ok(Bytes::new());
                    }
                    self.store
                        .get_range(&location, start..end)
                        .await
                        .map_err(|error| driver_error("read", error))
                }
            }
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "open")?;
            let result = self
                .store
                .get(&location)
                .await
                .map_err(|error| driver_error("open", error))?;
            let size = result.meta.size;
            let stream = result
                .into_stream()
                .map_err(|error| driver_error("open", error))
                .boxed();
            Ok(OpenedFile::new(size, stream))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "head")?;
            let meta = self
                .store
                .head(&location)
                .await
                .map_err(|error| driver_error("head", error))?;
            Ok(FileMeta {
                path: path.to_owned(),
                size: meta.size,
                last_modified: meta.last_modified,
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        Box::pin(async move {
            let location = file_location(path, "delete")?;
            self.store
                .head(&location)
                .await
                .map_err(|error| driver_error("delete", error))?;
            self.store
                .delete(&location)
                .await
                .map_err(|error| driver_error("delete", error))
        })
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> {
        Box::pin(async move {
            let target = list_location(prefix)?;
            let folder = (!prefix.is_empty()).then(|| format!("{prefix}/"));
            let entries: Vec<ObjectMeta> = self
                .store
                .list(target.as_ref())
                .try_collect()
                .await
                .map_err(|error| driver_error("list", error))?;
            Ok(entries
                .into_iter()
                .filter(|meta| {
                    let path = meta.location.as_ref();
                    !path.is_empty() && folder.as_deref().is_none_or(|dir| path.starts_with(dir))
                })
                .map(|meta| FileMeta {
                    path: meta.location.into(),
                    size: meta.size,
                    last_modified: meta.last_modified,
                })
                .collect())
        })
    }
}

/// Multipart upload in progress: parts stay hidden until [`DiskUpload::finish`]
/// completes, and [`DiskUpload::abort`] or dropping leaves no file behind.
struct GodamUpload {
    upload: Option<Box<dyn MultipartUpload>>,
}

impl DiskUpload for GodamUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        Box::pin(async move {
            if chunk.is_empty() {
                return Ok(());
            }
            let Some(mut upload) = self.upload.take() else {
                return Err(operation_failed("upload"));
            };
            match upload.put_part(chunk.into()).await {
                Ok(()) => {
                    self.upload = Some(upload);
                    Ok(())
                }
                Err(error) => {
                    let _ = upload.abort().await;
                    Err(driver_error("upload", error))
                }
            }
        })
    }

    fn finish(mut self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        let upload = self.upload.take();
        Box::pin(async move {
            let Some(mut upload) = upload else {
                return Err(operation_failed("upload"));
            };
            match upload.complete().await {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = upload.abort().await;
                    Err(driver_error("upload", error))
                }
            }
        })
    }

    fn abort(mut self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        let upload = self.upload.take();
        Box::pin(async move {
            let Some(mut upload) = upload else {
                return Ok(());
            };
            upload
                .abort()
                .await
                .map_err(|error| driver_error("upload", error))
        })
    }
}
