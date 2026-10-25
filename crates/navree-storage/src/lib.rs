//! # navree-storage
//!
//! `StorageModule`: named disks, streaming, signed temporary URLs and upload
//! checks, on [object_store](https://docs.rs/object_store) (local filesystem
//! and in-memory drivers).
//!
//! - [`StorageModule`] is global and exports [`Storage`] (every disk, by
//!   name) and [`Disk`] (the default disk). Configure it with
//!   [`StorageModule::for_root`] or [`StorageModule::from_config`] (section
//!   `storage`, see [`StorageOptions`]).
//! - A [`Disk`] puts, gets, streams, lists and deletes files. Paths are
//!   relative to the disk root and normalised (`\` is a separator, `.` and
//!   empty segments are dropped); `..`, segments made only of dots or spaces
//!   and control characters are rejected with [`StorageError::InvalidPath`],
//!   so no path can leave the root.
//! - [`Disk::url`] gives the public URL of a file on a disk with a
//!   `public_url`; [`Disk::temporary_url`] gives a link signed with
//!   HMAC-SHA-256 over `disk/path/expires` (key derived from the app key),
//!   served by the public route `GET /storage/{disk}/{*path}`, which checks
//!   the signature and the expiry (`dyn Clock` aware) and streams the file
//!   with a content type guessed from its extension.
//! - [`store_upload`] streams a multipart file field to a disk under a random
//!   name, with the size limit and allowed types of [`UploadRules`], sniffing
//!   the type from the content (413 / 400 error responses).
//! - A `storage` readiness check (`dyn HealthCheck`) writes, reads and
//!   deletes a probe file on every disk.
//!
//! ```
//! use chrono::{Duration, Utc};
//! use navree_storage::{DiskConfig, Storage, StorageOptions};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_storage::StorageError> {
//! let mut options = StorageOptions::default();
//! options.default = "scratch".into();
//! options.disks.clear();
//! options.disks.insert("scratch".into(), DiskConfig::Memory);
//! // In an app, resolve `Arc<Storage>` or `Arc<Disk>` instead.
//! let storage = Storage::new(&options, b"signing key")?;
//!
//! let disk = storage.default_disk();
//! disk.put("reports/2026/q1.csv", "region,total\nemea,42\n").await?;
//! assert_eq!(disk.list("reports").await?, vec!["reports/2026/q1.csv"]);
//! assert_eq!(disk.size("reports/2026/q1.csv").await?, 21);
//! assert!(disk.put("../escape.txt", "no").await.is_err());
//!
//! let link = disk.temporary_url("reports/2026/q1.csv", Utc::now() + Duration::minutes(5))?;
//! assert!(link.starts_with("/storage/scratch/reports/2026/q1.csv?expires="));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod disk;
mod error;
mod module;
mod options;
mod path;
mod serve;
mod signing;
mod storage;
mod upload;

pub use disk::{ByteStream, Disk};
pub use error::StorageError;
pub use module::StorageModule;
pub use options::{DiskConfig, StorageOptions};
pub use storage::Storage;
pub use upload::{StoredFile, UploadRules, store_upload};
