//! Disk drivers a module registers under a name: [`DiskDriver`] (with
//! [`DiskUpload`], [`OpenedFile`] and [`DiskDriverError`]), and
//! [`ModuleBuilderStorageExt`], which registers one.
//!
//! A disk whose `driver` names a registered driver is served by it: the
//! [`Disk`](crate::Disk) in front checks and normalises every path first,
//! so the driver only ever receives `/`-separated relative paths with no
//! empty, `.` or `..` segment and no control character.

use std::fmt;
use std::future::Future;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::stream::BoxStream;
use navree_config::__private::register_driver;
use navree_config::DriverContext;
use navree_kernel::{BoxError, BoxFuture, ModuleBuilder};

use crate::FileMeta;
use crate::options::SLOT;

/// The file store behind a disk, as a module provides it:
/// `m.disk_driver("sanduk", ..)` registers one, and a disk with
/// `driver = "sanduk"` is served by it.
///
/// Every path the driver receives is already normalised: relative to the
/// disk root, `/`-separated, with no empty, `.` or `..` segment and no
/// control character. Every method is the whole operation: the disk does
/// not retry, so do not retry inside the driver either, and say what
/// failed in [`DiskDriverError::Failed`] without the file's contents.
///
/// A disk of a registered driver has no URL provider: its temporary URLs
/// and direct uploads go through the app's own signed routes, as a memory
/// disk's do.
///
/// ```
/// use std::collections::BTreeMap;
/// use std::ops::Range;
/// use std::sync::{Arc, Mutex};
///
/// use bytes::{Bytes, BytesMut};
/// use chrono::{DateTime, Utc};
/// use futures_util::stream;
/// use navree_kernel::BoxFuture;
/// use navree_storage::{DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile};
///
/// type Files = Arc<Mutex<BTreeMap<String, (Bytes, DateTime<Utc>)>>>;
///
/// /// Files in process memory.
/// #[derive(Clone, Default)]
/// struct Sanduk {
///     files: Files,
/// }
///
/// impl Sanduk {
///     fn file(&self, path: &str) -> Result<(Bytes, DateTime<Utc>), DiskDriverError> {
///         let files = self.files.lock().unwrap();
///         files.get(path).cloned().ok_or(DiskDriverError::NotFound)
///     }
/// }
///
/// impl DiskDriver for Sanduk {
///     fn describe(&self) -> String {
///         "sanduk disk in memory".to_owned()
///     }
///
///     fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
///         self.files.lock().unwrap().insert(path.to_owned(), (contents, Utc::now()));
///         Box::pin(async { Ok(()) })
///     }
///
///     fn begin_upload<'a>(
///         &'a self,
///         path: &'a str,
///     ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> {
///         let upload = SandukUpload {
///             files: self.files.clone(),
///             path: path.to_owned(),
///             buffer: BytesMut::new(),
///         };
///         Box::pin(async move { Ok(Box::new(upload) as Box<dyn DiskUpload>) })
///     }
///
///     fn read<'a>(
///         &'a self,
///         path: &'a str,
///         range: Option<Range<u64>>,
///     ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> {
///         let read = self.file(path).map(|(contents, _)| match range {
///             // A range is cut short at the end of the file.
///             Some(range) => {
///                 let end = usize::try_from(range.end).map_or(contents.len(), |end| end.min(contents.len()));
///                 let start = usize::try_from(range.start).map_or(end, |start| start.min(end));
///                 contents.slice(start..end)
///             }
///             None => contents,
///         });
///         Box::pin(async move { read })
///     }
///
///     fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> {
///         let opened = self.file(path).map(|(contents, _)| {
///             let size = contents.len() as u64;
///             OpenedFile::new(size, Box::pin(stream::once(async move { Ok(contents) })))
///         });
///         Box::pin(async move { opened })
///     }
///
///     fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> {
///         let meta = self.file(path).map(|(contents, last_modified)| FileMeta {
///             path: path.to_owned(),
///             size: contents.len() as u64,
///             last_modified,
///         });
///         Box::pin(async move { meta })
///     }
///
///     fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>> {
///         let removed = self.files.lock().unwrap().remove(path);
///         Box::pin(async move { removed.map(drop).ok_or(DiskDriverError::NotFound) })
///     }
///
///     fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> {
///         // A prefix is a folder: `docs` holds `docs/a.txt`, not `docs2/b.txt`.
///         let folder = format!("{prefix}/");
///         let files: Vec<FileMeta> = self
///             .files
///             .lock()
///             .unwrap()
///             .iter()
///             .filter(|(path, _)| prefix.is_empty() || path.starts_with(&folder))
///             .map(|(path, (contents, last_modified))| FileMeta {
///                 path: path.clone(),
///                 size: contents.len() as u64,
///                 last_modified: *last_modified,
///             })
///             .collect();
///         Box::pin(async move { Ok(files) })
///     }
/// }
///
/// /// A file being written: it appears only when it is finished.
/// struct SandukUpload {
///     files: Files,
///     path: String,
///     buffer: BytesMut,
/// }
///
/// impl DiskUpload for SandukUpload {
///     fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
///         self.buffer.extend_from_slice(&chunk);
///         Box::pin(async { Ok(()) })
///     }
///
///     fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
///         let SandukUpload { files, path, buffer } = *self;
///         files.lock().unwrap().insert(path, (buffer.freeze(), Utc::now()));
///         Box::pin(async { Ok(()) })
///     }
///
///     fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
///         Box::pin(async { Ok(()) })
///     }
/// }
/// ```
pub trait DiskDriver: Send + Sync + 'static {
    /// What the disk is, for logs and errors: `sftp disk at
    /// files.ludhiana.example`. Never a password or a token.
    fn describe(&self) -> String;

    /// Writes `contents` to `path`, replacing any file there; readers see
    /// the old file or the new one, never part of one. The path is already
    /// normalised; do the whole operation; do not retry.
    fn put<'a>(&'a self, path: &'a str, contents: Bytes)
    -> BoxFuture<'a, Result<(), DiskDriverError>>;

    /// Starts writing a file chunk by chunk. The file appears at `path`
    /// only when the upload is [finished](DiskUpload::finish), and not at
    /// all when it is [aborted](DiskUpload::abort). The path is already
    /// normalised; do the whole operation; do not retry.
    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>>;

    /// The bytes of `path`, or of `range` of it, cut short at the end of
    /// the file (a range past the end is empty). A missing file is
    /// [`DiskDriverError::NotFound`]. The path is already normalised; do the
    /// whole operation; do not retry.
    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>>;

    /// Opens `path` for streaming: its size and its contents. A missing
    /// file is [`DiskDriverError::NotFound`]. The path is already
    /// normalised; do the whole operation; do not retry.
    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>>;

    /// The size and modification time of `path` (its [`FileMeta::path`] is
    /// `path`). A missing file is [`DiskDriverError::NotFound`]. The path is
    /// already normalised; do the whole operation; do not retry.
    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>>;

    /// Deletes `path`; [`DiskDriverError::NotFound`] when there is none.
    /// The path is already normalised; do the whole operation; do not
    /// retry.
    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>>;

    /// Every file under the folder `prefix`, at any depth, in any order,
    /// each with its path from the disk root: `""` is the root, `docs`
    /// holds `docs/a.txt` but not `docs2/b.txt`, and a folder that does not
    /// exist has none. The prefix is already normalised; do the whole
    /// operation; do not retry.
    fn list<'a>(&'a self, prefix: &'a str)
    -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>>;
}

/// A file a [`DiskDriver`] is writing, chunk by chunk: what
/// [`DiskDriver::begin_upload`] returns. The disk drops it after
/// [`finish`](DiskUpload::finish) or [`abort`](DiskUpload::abort); one
/// of them is always called unless the task is cancelled.
///
/// ```
/// use bytes::{Bytes, BytesMut};
/// use navree_kernel::BoxFuture;
/// use navree_storage::{DiskDriverError, DiskUpload};
///
/// /// Collects the chunks, and hands the file over when it is finished.
/// struct Collect {
///     buffer: BytesMut,
/// }
///
/// impl DiskUpload for Collect {
///     fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
///         self.buffer.extend_from_slice(&chunk);
///         Box::pin(async { Ok(()) })
///     }
///
///     fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
///         let file: Bytes = self.buffer.freeze();
///         // Publish `file` at its path here.
///         drop(file);
///         Box::pin(async { Ok(()) })
///     }
///
///     fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
///         Box::pin(async { Ok(()) })
///     }
/// }
/// ```
pub trait DiskUpload: Send {
    /// Appends `chunk`.
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>>;

    /// Publishes the file: from now on readers see it whole.
    fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>>;

    /// Drops what was written: nothing appears at the path.
    fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>>;
}

/// A file a [`DiskDriver`] opened for streaming: its size in bytes and its
/// contents.
///
/// ```
/// use bytes::Bytes;
/// use futures_util::stream;
/// use navree_storage::OpenedFile;
///
/// let contents = Bytes::from_static(b"invoice,total\nINV-1101,100000\n");
/// let file = OpenedFile::new(
///     contents.len() as u64,
///     Box::pin(stream::once(async move { Ok(contents) })),
/// );
/// assert_eq!(file.size, 30);
/// ```
#[non_exhaustive]
pub struct OpenedFile {
    /// The size of the whole file in bytes.
    pub size: u64,
    /// The contents, in chunks.
    pub stream: BoxStream<'static, Result<Bytes, DiskDriverError>>,
}

impl OpenedFile {
    /// A file of `size` bytes whose contents are `stream`.
    pub fn new(size: u64, stream: BoxStream<'static, Result<Bytes, DiskDriverError>>) -> OpenedFile {
        OpenedFile { size, stream }
    }
}

/// The size; never the contents.
impl fmt::Debug for OpenedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedFile")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// How a [`DiskDriver`] operation fails.
///
/// ```
/// use navree_storage::DiskDriverError;
///
/// assert_eq!(DiskDriverError::NotFound.to_string(), "not found");
/// let error = DiskDriverError::Failed("the Ludhiana file server refused the login".into());
/// assert_eq!(error.to_string(), "the Ludhiana file server refused the login");
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DiskDriverError {
    /// There is no such file: the disk answers with a not-found error.
    #[error("not found")]
    NotFound,
    /// The driver could not do it; the error says why, without the file's
    /// contents, a password or a token.
    #[error("{0}")]
    Failed(BoxError),
}

/// Registers disk drivers on a [`ModuleBuilder`]: `use
/// navree::plugin::prelude::*;` (or this trait) brings the method.
///
/// ```no_run
/// use std::ops::Range;
///
/// use bytes::Bytes;
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_kernel::{BoxFuture, Module, ModuleBuilder, Navree};
/// use navree_storage::{
///     DiskDriver, DiskDriverError, DiskUpload, FileMeta, ModuleBuilderStorageExt, OpenedFile,
///     StorageModule,
/// };
/// use serde::Deserialize;
///
/// /// The settings of a `sanduk` disk: its own table, `[storage.disks.{disk}]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct SandukSettings {
///     host: String,
/// }
///
/// /// A file server's disk (every method talks to `host`).
/// struct Sanduk {
///     host: String,
/// }
///
/// impl DiskDriver for Sanduk {
///     fn describe(&self) -> String {
///         format!("sanduk disk at {}", self.host)
///     }
///     # fn put<'a>(&'a self, _: &'a str, _: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> { todo!() }
///     # fn begin_upload<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> { todo!() }
///     # fn read<'a>(&'a self, _: &'a str, _: Option<Range<u64>>) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> { todo!() }
///     # fn open<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> { todo!() }
///     # fn head<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> { todo!() }
///     # fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>> { todo!() }
///     # fn list<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> { todo!() }
///     // put, begin_upload, read, open, head, delete and list
/// }
///
/// struct SandukModule;
///
/// impl Module for SandukModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         // A disk with `driver = "sanduk"` is served by it.
///         m.disk_driver("sanduk", |ctx| async move {
///             let settings: SandukSettings = ctx.settings()?;
///             Ok(Sanduk { host: settings.host })
///         });
///     }
/// }
///
/// fn main() -> Result<(), navree_kernel::BoxError> {
///     Navree::app()
///         .module(ConfigModule::for_root(ConfigSources::defaults()))
///         .module(StorageModule::from_config())
///         .module(SandukModule)
///         .run()
/// }
/// ```
pub trait ModuleBuilderStorageExt {
    /// Registers a disk driver under `name` (a lower-case letter followed
    /// by up to 63 lower-case letters, digits or `_`): a disk with
    /// `driver = "{name}"` is served by it, and `factory` builds one driver
    /// per such disk at boot from its [`DriverContext`], whose settings are
    /// the disk's table (`storage.disks.{disk}`, `driver` included, so a
    /// settings type ignores the keys it does not know) and whose
    /// [`instance`](DriverContext::instance) is the disk's name.
    ///
    /// A name that is not such a word, a built-in driver (`local`,
    /// `memory`, `s3`, `gcs`, `azure`) and a name two modules register
    /// fail the boot, naming `m.disk_driver` and the module, also when no
    /// disk uses it; `config:check` says the same. A disk naming a driver no
    /// module registers, and a factory that fails, fail the boot too.
    fn disk_driver<D, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        D: DiskDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<D, BoxError>> + Send + 'static;
}

impl ModuleBuilderStorageExt for ModuleBuilder {
    fn disk_driver<D, F, Fut>(&mut self, name: &'static str, factory: F) -> &mut Self
    where
        D: DiskDriver,
        F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<D, BoxError>> + Send + 'static,
    {
        // No settings claim by name: the disk's table is claimed whole when
        // its driver is registered.
        register_driver::<dyn DiskDriver, _, _>(self, &SLOT, None, name, move |ctx| {
            let built = factory(ctx);
            async move {
                built
                    .await
                    .map(|driver| Arc::new(driver) as Arc<dyn DiskDriver>)
            }
        })
    }
}
