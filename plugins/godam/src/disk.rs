//! [`GodamDisk`]: a [`DiskDriver`] backed by a [`Store`].

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use futures_util::{StreamExt, TryStreamExt};
use navree::kernel::BoxFuture;
use navree::prelude::BoxError;
use navree::storage::store::{MemoryStore, PrefixedStore, Store, StoreError, Upload};
use navree::storage::{DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile};

use crate::GodamSettings;

/// A [`DiskDriver`] over a [`Store`], built by default as [`PrefixedStore`]
/// around [`MemoryStore`].
#[derive(Clone)]
pub struct GodamDisk {
    store: Arc<dyn Store>,
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
        let memory: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let store: Arc<dyn Store> = if prefix.is_empty() {
            memory
        } else {
            Arc::new(PrefixedStore::new(memory, prefix))
        };
        Ok(GodamDisk { store })
    }

    /// Wraps an existing [`Store`] directly as a `godam` disk.
    pub fn from_store(store: Arc<dyn Store>) -> GodamDisk {
        GodamDisk { store }
    }

    /// Wraps an existing [`Store`] under `settings.prefix`.
    pub fn with_store(
        store: Arc<dyn Store>,
        settings: GodamSettings,
    ) -> Result<GodamDisk, BoxError> {
        let prefix = settings.parse_prefix(None)?;
        let store: Arc<dyn Store> = if prefix.is_empty() {
            store
        } else {
            Arc::new(PrefixedStore::new(store, prefix))
        };
        Ok(GodamDisk { store })
    }
}

/// Minimum multipart chunk size (5 MiB), matching S3 and Cloudflare R2 rules
/// for every part before the final tail part.
const PART_BYTES: usize = 5 * 1024 * 1024;

fn operation_failed(operation: &'static str) -> DiskDriverError {
    DiskDriverError::Failed(format!("godam {operation} failed").into())
}

fn driver_error(operation: &'static str, error: StoreError) -> DiskDriverError {
    match error {
        StoreError::NotFound => DiskDriverError::NotFound,
        _ => operation_failed(operation),
    }
}

fn file_path<'a>(path: &'a str, operation: &'static str) -> Result<&'a str, DiskDriverError> {
    if path.is_empty()
        || path.len() > 1024
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment.chars().all(|c| c == '.' || c == ' '))
    {
        return Err(operation_failed(operation));
    }
    Ok(path)
}

fn list_prefix(prefix: &str) -> Result<&str, DiskDriverError> {
    if prefix.is_empty() {
        return Ok("");
    }
    file_path(prefix, "list")
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
            let path = file_path(path, "put")?;
            self.store
                .put(path, contents)
                .await
                .map_err(|error| driver_error("put", error))
        })
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> {
        Box::pin(async move {
            let path = file_path(path, "upload")?.to_owned();
            Ok(Box::new(GodamUpload {
                store: self.store.clone(),
                path,
                upload: None,
                buffer: BytesMut::new(),
                closed: false,
            }) as Box<dyn DiskUpload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> {
        Box::pin(async move {
            let path = file_path(path, "read")?;
            match range {
                None => self
                    .store
                    .read(path, None)
                    .await
                    .map_err(|error| driver_error("read", error)),
                Some(range) => {
                    let meta = self
                        .store
                        .head(path)
                        .await
                        .map_err(|error| driver_error("read", error))?;
                    let end = range.end.min(meta.size);
                    let start = range.start.min(end);
                    if start == end {
                        return Ok(Bytes::new());
                    }
                    self.store
                        .read(path, Some(start..end))
                        .await
                        .map_err(|error| driver_error("read", error))
                }
            }
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> {
        Box::pin(async move {
            let path = file_path(path, "open")?;
            let object = self
                .store
                .open(path)
                .await
                .map_err(|error| driver_error("open", error))?;
            let size = object.size;
            let stream = object
                .stream
                .map_err(|error| driver_error("open", error))
                .boxed();
            Ok(OpenedFile::new(size, stream))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> {
        Box::pin(async move {
            let path = file_path(path, "head")?;
            let meta = self
                .store
                .head(path)
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
            let path = file_path(path, "delete")?;
            self.store
                .delete(path)
                .await
                .map_err(|error| driver_error("delete", error))
        })
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> {
        Box::pin(async move {
            let target = list_prefix(prefix)?;
            let folder = (!target.is_empty()).then(|| format!("{target}/"));
            let entries = self
                .store
                .list(target)
                .await
                .map_err(|error| driver_error("list", error))?;
            Ok(entries
                .into_iter()
                .filter(|meta| {
                    !meta.path.is_empty()
                        && folder
                            .as_deref()
                            .is_none_or(|dir| meta.path.starts_with(dir))
                })
                .map(|meta| FileMeta {
                    path: meta.path,
                    size: meta.size,
                    last_modified: meta.last_modified,
                })
                .collect())
        })
    }
}

/// Buffered upload in progress: buffers up to 5 MiB before starting a store
/// upload so small or empty streams use a single `put` and large streams emit
/// uniform 5 MiB non-final parts.
struct GodamUpload {
    store: Arc<dyn Store>,
    path: String,
    upload: Option<Box<dyn Upload>>,
    buffer: BytesMut,
    closed: bool,
}

impl DiskUpload for GodamUpload {
    fn write<'a>(&'a mut self, mut chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        Box::pin(async move {
            if self.closed {
                return Err(operation_failed("upload"));
            }
            while !chunk.is_empty() {
                let remaining = PART_BYTES - self.buffer.len();
                let take = chunk.len().min(remaining);
                self.buffer.extend_from_slice(&chunk.split_to(take));

                if self.buffer.len() == PART_BYTES {
                    if self.upload.is_none() {
                        match self.store.begin_upload(&self.path).await {
                            Ok(started) => self.upload = Some(started),
                            Err(error) => {
                                self.buffer.clear();
                                self.closed = true;
                                return Err(driver_error("upload", error));
                            }
                        }
                    }
                    let part = self.buffer.split().freeze();
                    let mut upload = self.upload.take().expect("upload initialized");
                    match upload.write(part).await {
                        Ok(()) => {
                            self.upload = Some(upload);
                        }
                        Err(error) => {
                            self.buffer.clear();
                            self.closed = true;
                            let _ = upload.abort().await;
                            return Err(driver_error("upload", error));
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn finish(mut self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        Box::pin(async move {
            if self.closed {
                return Err(operation_failed("upload"));
            }
            self.closed = true;
            match self.upload.take() {
                None => {
                    let payload = self.buffer.freeze();
                    self.store
                        .put(&self.path, payload)
                        .await
                        .map_err(|error| driver_error("upload", error))
                }
                Some(mut upload) => {
                    if !self.buffer.is_empty() {
                        let tail = self.buffer.freeze();
                        if let Err(error) = upload.write(tail).await {
                            let _ = upload.abort().await;
                            return Err(driver_error("upload", error));
                        }
                    }
                    upload
                        .finish()
                        .await
                        .map_err(|error| driver_error("upload", error))
                }
            }
        })
    }

    fn abort(mut self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        self.buffer.clear();
        self.closed = true;
        let upload = self.upload.take();
        Box::pin(async move {
            let Some(upload) = upload else {
                return Ok(());
            };
            upload
                .abort()
                .await
                .map_err(|error| driver_error("upload", error))
        })
    }
}
