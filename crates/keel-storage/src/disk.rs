//! [`Disk`]: one named file store.

use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use futures_util::{Stream, StreamExt, TryStreamExt};
use keel_kernel::BoxError;
use object_store::path::Path as StorePath;
use object_store::{ObjectStore, ObjectStoreExt, WriteMultipart};

use crate::StorageError;
use crate::path::normalize;
use crate::signing::{Signer, encode_path};

/// A stream of file contents, as returned by [`Disk::get_stream`].
pub type ByteStream = BoxStream<'static, Result<Bytes, StorageError>>;

/// Parts uploaded concurrently by [`Disk::put_stream`].
const STREAM_CONCURRENCY: usize = 2;

/// One named file store (a local directory or memory). Cheap to clone.
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
    store: Arc<dyn ObjectStore>,
    /// The public URL of the root, without a trailing `/`.
    public_url: Option<String>,
    /// The base of temporary URLs, without a trailing `/` (may be empty).
    base_url: String,
    signer: Arc<Signer>,
}

impl Disk {
    pub(crate) fn new(
        name: String,
        store: Arc<dyn ObjectStore>,
        public_url: Option<&str>,
        base_url: Option<&str>,
        signer: Arc<Signer>,
    ) -> Self {
        Self {
            inner: Arc::new(DiskInner {
                name,
                store,
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

    /// Writes `contents` to `path`, replacing any existing file. The write
    /// is atomic: readers see the old or the new file, never a partial one.
    pub async fn put(&self, path: &str, contents: impl Into<Bytes>) -> Result<(), StorageError> {
        let location = self.location(path)?;
        self.inner
            .store
            .put(&location, contents.into().into())
            .await
            .map_err(|error| self.error(&location, error))?;
        Ok(())
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
        let location = self.location(path)?;
        let upload = self
            .inner
            .store
            .put_multipart(&location)
            .await
            .map_err(|error| self.error(&location, error))?;
        let mut writer = WriteMultipart::new(upload);
        let mut stream = pin!(stream);
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(source) => {
                    abort(writer, &self.inner.name).await;
                    return Err(StorageError::Stream {
                        source: source.into(),
                    });
                }
            };
            if let Err(error) = writer.wait_for_capacity(STREAM_CONCURRENCY).await {
                abort(writer, &self.inner.name).await;
                return Err(self.error(&location, error));
            }
            written += chunk.len() as u64;
            writer.put(chunk);
        }
        writer
            .finish()
            .await
            .map_err(|error| self.error(&location, error))?;
        Ok(written)
    }

    /// The contents of `path`.
    pub async fn get(&self, path: &str) -> Result<Bytes, StorageError> {
        let location = self.location(path)?;
        let result = self
            .inner
            .store
            .get(&location)
            .await
            .map_err(|error| self.error(&location, error))?;
        result
            .bytes()
            .await
            .map_err(|error| self.error(&location, error))
    }

    /// The contents of `path` as a stream of chunks (local files are read
    /// on Tokio's blocking pool).
    pub async fn get_stream(&self, path: &str) -> Result<ByteStream, StorageError> {
        Ok(self.open(path).await?.1)
    }

    /// The size of `path` and a stream of its contents.
    pub(crate) async fn open(&self, path: &str) -> Result<(u64, ByteStream), StorageError> {
        let location = self.location(path)?;
        let result = self
            .inner
            .store
            .get(&location)
            .await
            .map_err(|error| self.error(&location, error))?;
        let size = result.meta.size;
        let disk = self.clone();
        let stream = result
            .into_stream()
            .map_err(move |error| disk.error(&location, error))
            .boxed();
        Ok((size, stream))
    }

    /// Whether `path` exists.
    pub async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        let location = self.location(path)?;
        match self.inner.store.head(&location).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(error) => Err(self.error(&location, error)),
        }
    }

    /// Deletes `path`; deleting a missing file succeeds.
    pub async fn delete(&self, path: &str) -> Result<(), StorageError> {
        let location = self.location(path)?;
        match self.inner.store.delete(&location).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(self.error(&location, error)),
        }
    }

    /// The size of `path` in bytes.
    pub async fn size(&self, path: &str) -> Result<u64, StorageError> {
        let location = self.location(path)?;
        self.inner
            .store
            .head(&location)
            .await
            .map(|meta| meta.size)
            .map_err(|error| self.error(&location, error))
    }

    /// Every file under the directory `prefix` (recursively), sorted.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let prefix = store_path(prefix, normalize(prefix, true)?)?;
        let mut paths: Vec<String> = self
            .inner
            .store
            .list(Some(&prefix))
            .map_ok(|meta| meta.location.as_ref().to_owned())
            .try_collect()
            .await
            .map_err(|error| self.error(&prefix, error))?;
        paths.sort();
        Ok(paths)
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

    /// A link that downloads `path` until `expires_at`, without a session:
    /// `{base_url}/storage/{disk}/{path}?expires={unix}&signature={hex}`.
    /// The signature is an HMAC-SHA-256 of `disk/path/expires`; the route
    /// `GET /storage/{disk}/{*path}` (mounted when the app has `HttpModule`)
    /// checks it and the expiry and streams the file.
    pub fn temporary_url(
        &self,
        path: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<String, StorageError> {
        let path = normalize(path, false)?;
        let expires = expires_at.timestamp();
        let signature = self.inner.signer.sign(&self.inner.name, &path, expires);
        Ok(format!(
            "{}/storage/{}/{}?expires={expires}&signature={signature}",
            self.inner.base_url,
            self.inner.name,
            encode_path(&path),
        ))
    }

    /// The signer of temporary URLs.
    pub(crate) fn signer(&self) -> &Signer {
        &self.inner.signer
    }

    /// The store location of `path`.
    fn location(&self, path: &str) -> Result<StorePath, StorageError> {
        store_path(path, normalize(path, false)?)
    }

    /// Maps a backend error on `location`.
    fn error(&self, location: &StorePath, error: object_store::Error) -> StorageError {
        match error {
            object_store::Error::NotFound { .. } => StorageError::NotFound {
                disk: self.inner.name.clone(),
                path: location.as_ref().to_owned(),
            },
            source => StorageError::Backend {
                disk: self.inner.name.clone(),
                source,
            },
        }
    }
}

/// The store path of a normalised path (`given` is the path as given, for errors).
fn store_path(given: &str, normalized: String) -> Result<StorePath, StorageError> {
    StorePath::parse(normalized).map_err(|_| StorageError::InvalidPath {
        path: given.to_owned(),
        reason: "not a valid path",
    })
}

/// Aborts a multipart upload, logging (not returning) a failure to clean up.
async fn abort(writer: WriteMultipart, disk: &str) {
    if let Err(error) = writer.abort().await {
        tracing::warn!(disk, error = %error, "could not clean up an aborted upload");
    }
}

impl fmt::Debug for Disk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Disk")
            .field("name", &self.inner.name)
            .field("store", &self.inner.store.to_string())
            .field("public_url", &self.inner.public_url)
            .finish()
    }
}
