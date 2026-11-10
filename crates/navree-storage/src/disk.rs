//! [`Disk`]: one named file store.

use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use futures_util::{Stream, StreamExt, TryStreamExt};
use navree_contracts::HealthStatus;
use navree_crypto::random_token;
use navree_kernel::BoxError;
use navree_telemetry::StorageOperation;
use serde::Serialize;

use crate::StorageError;
use crate::drivers::Backend;
use crate::path::normalize;
use crate::provider::Provider;
use crate::signing::{Signer, encode_path};
use crate::store::{Meta, Store, StoreError, Upload};

/// A stream of file contents, as returned by [`Disk::get_stream`].
pub type ByteStream = BoxStream<'static, Result<Bytes, StorageError>>;

/// A file's path, size and last modification time, as returned by
/// [`Disk::metadata`] and [`Disk::files`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FileMeta {
    /// The path on the disk (relative to its root and prefix).
    pub path: String,
    /// The size in bytes.
    pub size: u64,
    /// When the file was last written.
    pub last_modified: DateTime<Utc>,
}

/// One named file store: a local directory, memory, or a bucket of S3,
/// Google Cloud Storage or Azure. Cheap to clone.
///
/// Paths are relative to the disk root. Every method normalises them first:
/// `\` is a separator like `/`, and leading, trailing or repeated separators
/// and `.` segments are dropped (`/a//./b.txt` is `a/b.txt`). `..`, other
/// segments made only of dots or spaces, control characters and paths over
/// 1024 bytes fail with [`StorageError::InvalidPath`] before reaching the
/// backend, so no path can leave the disk root.
#[derive(Clone)]
pub struct Disk {
    inner: Arc<DiskInner>,
}

struct DiskInner {
    name: String,
    /// The driver name (`local`, `memory`, `s3`, `gcs`, `azure`).
    driver: &'static str,
    store: Arc<dyn Store>,
    /// Signs links for cloud disks; local and memory disks have none and
    /// use the app's own routes.
    provider: Option<Provider>,
    /// The public URL of the root, without a trailing `/`.
    public_url: Option<String>,
    /// The base of the app's own links, without a trailing `/` (may be empty).
    base_url: String,
    signer: Arc<Signer>,
}

impl Disk {
    pub(crate) fn new(
        name: String,
        driver: &'static str,
        backend: Backend,
        public_url: Option<&str>,
        base_url: Option<&str>,
        signer: Arc<Signer>,
    ) -> Self {
        Self {
            inner: Arc::new(DiskInner {
                name,
                driver,
                store: backend.store,
                provider: backend.provider,
                public_url: public_url.map(|url| url.trim_end_matches('/').to_owned()),
                base_url: base_url
                    .map(|url| url.trim_end_matches('/').to_owned())
                    .unwrap_or_default(),
                signer,
            }),
        }
    }

    /// The disk name.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// The driver: `local`, `memory`, `s3`, `gcs` or `azure`.
    pub fn driver(&self) -> &'static str {
        self.inner.driver
    }

    /// Writes `contents` to `path`, replacing any existing file. The write
    /// is atomic: readers see the old or the new file, never a partial one.
    pub async fn put(&self, path: &str, contents: impl Into<Bytes>) -> Result<(), StorageError> {
        let location = self.location(path)?;
        let contents = contents.into();
        let bytes = contents.len() as u64;
        let started = std::time::Instant::now();
        let result = self
            .inner
            .store
            .put(&location, contents)
            .await
            .map_err(|error| self.error(&location, error));
        self.report(
            StorageOperation::Write,
            &location,
            started,
            Some(bytes),
            result.as_ref().err(),
        );
        result
    }

    /// Writes a stream of chunks to `path` without buffering the whole file
    /// and returns the number of bytes written. The file appears only when
    /// the stream ends successfully; if the stream fails, nothing is stored
    /// and the error is [`StorageError::Stream`].
    pub async fn put_stream<S, E>(&self, path: &str, stream: S) -> Result<u64, StorageError>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<BoxError>,
    {
        let started = std::time::Instant::now();
        let result = self.write_stream(path, stream).await;
        let location = self.location(path).unwrap_or_else(|_| path.to_owned());
        self.report(
            StorageOperation::Write,
            &location,
            started,
            result.as_ref().ok().copied(),
            result.as_ref().err(),
        );
        result
    }

    /// [`put_stream`](Self::put_stream) without the report.
    async fn write_stream<S, E>(&self, path: &str, stream: S) -> Result<u64, StorageError>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: Into<BoxError>,
    {
        let location = self.location(path)?;
        let mut upload = self
            .inner
            .store
            .begin_upload(&location)
            .await
            .map_err(|error| self.error(&location, error))?;
        let mut stream = pin!(stream);
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(source) => {
                    abort(upload, &self.inner.name).await;
                    return Err(StorageError::Stream {
                        source: source.into(),
                    });
                }
            };
            let length = chunk.len() as u64;
            if let Err(error) = upload.write(chunk).await {
                abort(upload, &self.inner.name).await;
                return Err(self.error(&location, error));
            }
            written += length;
        }
        upload
            .finish()
            .await
            .map_err(|error| self.error(&location, error))?;
        Ok(written)
    }

    /// The contents of `path`.
    pub async fn get(&self, path: &str) -> Result<Bytes, StorageError> {
        let location = self.location(path)?;
        let started = std::time::Instant::now();
        let result = self
            .inner
            .store
            .read(&location, None)
            .await
            .map_err(|error| self.error(&location, error));
        self.report(
            StorageOperation::Read,
            &location,
            started,
            result.as_ref().ok().map(|bytes| bytes.len() as u64),
            result.as_ref().err(),
        );
        result
    }

    /// The contents of `path` as a stream of chunks (local files are read
    /// on Tokio's blocking pool).
    pub async fn get_stream(&self, path: &str) -> Result<ByteStream, StorageError> {
        Ok(self.open(path).await?.1)
    }

    /// The size of `path` and a stream of its contents.
    pub(crate) async fn open(&self, path: &str) -> Result<(u64, ByteStream), StorageError> {
        let location = self.location(path)?;
        let started = std::time::Instant::now();
        let object = self
            .inner
            .store
            .open(&location)
            .await
            .map_err(|error| self.error(&location, error));
        self.report(
            StorageOperation::Read,
            &location,
            started,
            object.as_ref().ok().map(|object| object.size),
            object.as_ref().err(),
        );
        let object = object?;
        let disk = self.clone();
        let stream = object
            .stream
            .map_err(move |error| disk.error(&location, error))
            .boxed();
        Ok((object.size, stream))
    }

    /// Whether `path` exists.
    pub async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        let location = self.location(path)?;
        match self.inner.store.head(&location).await {
            Ok(_) => Ok(true),
            Err(StoreError::NotFound) => Ok(false),
            Err(error) => Err(self.error(&location, error)),
        }
    }

    /// Deletes `path`; deleting a missing file succeeds.
    pub async fn delete(&self, path: &str) -> Result<(), StorageError> {
        let location = self.location(path)?;
        let started = std::time::Instant::now();
        let result = match self.inner.store.delete(&location).await {
            Ok(()) | Err(StoreError::NotFound) => Ok(()),
            Err(error) => Err(self.error(&location, error)),
        };
        self.report(
            StorageOperation::Delete,
            &location,
            started,
            None,
            result.as_ref().err(),
        );
        result
    }

    /// The size of `path` in bytes.
    pub async fn size(&self, path: &str) -> Result<u64, StorageError> {
        Ok(self.metadata(path).await?.size)
    }

    /// The size and last modification time of `path`.
    pub async fn metadata(&self, path: &str) -> Result<FileMeta, StorageError> {
        let location = self.location(path)?;
        self.inner
            .store
            .head(&location)
            .await
            .map(file_meta)
            .map_err(|error| self.error(&location, error))
    }

    /// Every file under the directory `prefix` (recursively), sorted.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        Ok(self
            .files(prefix)
            .await?
            .into_iter()
            .map(|file| file.path)
            .collect())
    }

    /// Every file under the directory `prefix` (recursively) with its size
    /// and last modification time, sorted by path.
    pub async fn files(&self, prefix: &str) -> Result<Vec<FileMeta>, StorageError> {
        let prefix = normalize(prefix, true)?;
        let started = std::time::Instant::now();
        let listed = self
            .inner
            .store
            .list(&prefix)
            .await
            .map_err(|error| self.error(&prefix, error));
        self.report(
            StorageOperation::List,
            &prefix,
            started,
            None,
            listed.as_ref().err(),
        );
        let mut files: Vec<FileMeta> = listed?.into_iter().map(file_meta).collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// The public URL of `path`: the disk's `public_url` followed by the
    /// percent-encoded path. Fails with [`StorageError::NoPublicUrl`] when
    /// the disk has none.
    pub fn url(&self, path: &str) -> Result<String, StorageError> {
        let path = normalize(path, false)?;
        let base = self
            .inner
            .public_url
            .as_deref()
            .ok_or_else(|| StorageError::NoPublicUrl {
                disk: self.inner.name.clone(),
            })?;
        Ok(format!("{base}/{}", encode_path(&path)))
    }

    /// A link that downloads `path` until `expires_at`, without a session.
    ///
    /// - On an S3, Google Cloud Storage or Azure disk the provider signs it,
    ///   and the client downloads straight from the bucket. S3 and Google
    ///   Cloud Storage links last at most 7 days.
    /// - On a local or memory disk it is the app's own link,
    ///   `{base_url}/storage/{disk}/{path}?expires={unix}&signature={hex}`:
    ///   an HMAC-SHA-256 of `disk/path/expires` that the route
    ///   `GET /storage/{disk}/{*path}` (mounted when the app has
    ///   `HttpModule`) checks before streaming the file.
    pub async fn temporary_url(
        &self,
        path: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, StorageError> {
        let path = normalize(path, false)?;
        if let Some(provider) = &self.inner.provider {
            let ttl = self.provider_ttl(provider, expires_at)?;
            return provider
                .get_url(&path, ttl)
                .await
                .map_err(|error| self.backend_error(error));
        }
        let expires = expires_at.timestamp();
        let signature = self.inner.signer.sign(&self.inner.name, &path, expires);
        Ok(format!(
            "{}/storage/{}/{}?expires={expires}&signature={signature}",
            self.inner.base_url,
            self.inner.name,
            encode_path(&path),
        ))
    }

    /// Readiness of this disk: writes a small file under `.navree-health/`,
    /// reads it back and deletes it. `Down` says which step failed.
    pub async fn check_health(&self) -> HealthStatus {
        let path = format!(".navree-health/{}", random_token(16));
        if let Err(error) = self.put(&path, "ok").await {
            return HealthStatus::Down(error.to_string());
        }
        let read = self.get(&path).await;
        if let Err(error) = self.delete(&path).await {
            return HealthStatus::Down(error.to_string());
        }
        match read {
            Ok(bytes) if bytes == "ok" => HealthStatus::Up,
            Ok(_) => {
                HealthStatus::Down("the disk returned other contents than just written".to_owned())
            }
            Err(error) => HealthStatus::Down(error.to_string()),
        }
    }

    /// How long a provider-signed link to `expires_at` lasts (at least a
    /// second), refused beyond the provider's limit.
    pub(crate) fn provider_ttl(
        &self,
        provider: &Provider,
        expires_at: DateTime<Utc>,
    ) -> Result<std::time::Duration, StorageError> {
        // Rounded up: a link asked for "in one hour" lasts 3600 seconds.
        let millis = (expires_at - Utc::now()).num_milliseconds();
        let seconds = millis.saturating_add(999).div_euclid(1000).max(1);
        let ttl = std::time::Duration::from_secs(seconds.unsigned_abs());
        match provider.max_ttl() {
            Some(max) if ttl > max => Err(StorageError::Signing {
                disk: self.inner.name.clone(),
                reason: format!(
                    "{} links last at most {} days",
                    self.inner.driver,
                    max.as_secs() / 86_400
                ),
            }),
            _ => Ok(ttl),
        }
    }

    /// The provider signing this disk's links, if it is a cloud disk.
    pub(crate) fn provider(&self) -> Option<&Provider> {
        self.inner.provider.as_ref()
    }

    /// The base of the app's own links (may be empty).
    pub(crate) fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// The store, for crate-internal operations.
    pub(crate) fn store(&self) -> &Arc<dyn Store> {
        &self.inner.store
    }

    /// The signer of the app's own links.
    pub(crate) fn signer(&self) -> &Signer {
        &self.inner.signer
    }

    /// The store location of `path`: its normalised form.
    pub(crate) fn location(&self, path: &str) -> Result<String, StorageError> {
        normalize(path, false)
    }

    /// Reports one operation on `location` to telemetry (inside a request,
    /// a job or a scheduled task): the disk, what was done, how many bytes,
    /// how long it took and how it ended; never the contents.
    fn report(
        &self,
        operation: StorageOperation,
        location: &str,
        started: std::time::Instant,
        bytes: Option<u64>,
        error: Option<&StorageError>,
    ) {
        navree_telemetry::Telemetry::current().storage_op(|| {
            let mut op = navree_telemetry::StorageOp::new(
                self.inner.name.as_str(),
                self.inner.driver,
                operation,
                location,
                started.elapsed(),
            );
            if let Some(bytes) = bytes {
                op = op.bytes(bytes);
            }
            match error {
                Some(StorageError::NotFound { .. }) => op.failed("not_found"),
                Some(_) => op.failed("failed"),
                None => op,
            }
        });
    }

    /// A backend error not tied to a path.
    pub(crate) fn backend_error(&self, source: BoxError) -> StorageError {
        StorageError::Backend {
            disk: self.inner.name.clone(),
            source,
        }
    }

    /// Maps a backend error on `location`.
    pub(crate) fn error(&self, location: &str, error: StoreError) -> StorageError {
        match error {
            StoreError::NotFound => StorageError::NotFound {
                disk: self.inner.name.clone(),
                path: location.to_owned(),
            },
            StoreError::Failed(source) => StorageError::Backend {
                disk: self.inner.name.clone(),
                source,
            },
        }
    }
}

/// A listing or `head` result as a [`FileMeta`].
fn file_meta(meta: Meta) -> FileMeta {
    FileMeta {
        path: meta.path,
        size: meta.size,
        last_modified: meta.last_modified,
    }
}

/// Aborts an upload, logging (not returning) a failure to clean up.
async fn abort(upload: Box<dyn Upload>, disk: &str) {
    if let Err(error) = upload.abort().await {
        tracing::warn!(disk, error = %error, "could not clean up an aborted upload");
    }
}

impl fmt::Debug for Disk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disk")
            .field("name", &self.inner.name)
            .field("driver", &self.inner.driver)
            .field("store", &self.inner.store.to_string())
            .field("public_url", &self.inner.public_url)
            .finish()
    }
}
