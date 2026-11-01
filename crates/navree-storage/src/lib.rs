//! # navree-storage
//!
//! `StorageModule`: named disks, streaming, signed links, direct uploads,
//! upload checks and disk sync.
//!
//! - Drivers: `local` (a directory) and `memory` (tests) always; `s3` (AWS,
//!   MinIO, Cloudflare R2, DigitalOcean Spaces), `gcs` and `azure` with the
//!   crate features of the same name (navree's `storage-s3`, `storage-gcs`
//!   and `storage-azure`), on rustls with ring.
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
//!   `public_url`. [`Disk::temporary_url`] gives an expiring download link:
//!   signed by the provider on cloud disks, and on local and memory disks an
//!   HMAC-SHA-256 link (key derived from the app key) served by the public
//!   route `GET /storage/{disk}/{*path}`.
//! - [`Disk::presigned_put`] signs a link a client uploads one file to
//!   directly, and [`Disk::accept_upload`] checks the file that arrived.
//!   [`store_upload`] streams a multipart file field to a disk instead.
//!   Both check size and the type sniffed from the content against
//!   [`UploadRules`].
//! - [`Storage::sync`] (and the `storage:sync` command) copies the files one
//!   disk lacks from another.
//! - A `storage` readiness check (`dyn HealthCheck`) writes, reads and
//!   deletes a probe file on every disk, the disks in parallel.
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
//! let link = disk
//!     .temporary_url("reports/2026/q1.csv", Utc::now() + Duration::minutes(5))
//!     .await?;
//! assert!(link.starts_with("/storage/scratch/reports/2026/q1.csv?expires="));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod disk;
mod drivers;
mod error;
mod module;
mod options;
mod path;
mod presign;
mod provider;
mod serve;
mod signing;
mod storage;
mod store;
mod sync;
mod upload;

pub use disk::{ByteStream, Disk, FileMeta};
pub use error::{StorageError, UploadRejection};
pub use module::StorageModule;
pub use options::{AzureConfig, DiskConfig, GcsConfig, S3Config, StorageOptions};
pub use presign::{PresignPut, PresignedUpload, upload_path};
pub use storage::Storage;
pub use sync::{SyncFailure, SyncOptions, SyncReport};
pub use upload::{StoredFile, UploadRules, store_upload};
